use bevy_ecs::prelude::*;
use voidmc_protocol::clientbound;

use crate::components::{
    ClientId, MinecraftEntityId, PlayerName, PlayerReady, PlayerUuid, Position, Rotation,
};
use crate::config::ServerConfigResource;
use crate::events::{PlayerQuitEvent, PlayerReadyEvent};
use crate::network::{NetworkChannels, OutgoingPacket};

/// Observer: when a player becomes ready, broadcast spawn info to/from all other ready players.
pub fn on_player_ready(
    event: On<PlayerReadyEvent>,
    channels: Res<NetworkChannels>,
    config: Res<ServerConfigResource>,
    new_player: Query<(
        &ClientId,
        &MinecraftEntityId,
        &PlayerUuid,
        &PlayerName,
        &Position,
        &Rotation,
    )>,
    all_players: Query<
        (
            &ClientId,
            &MinecraftEntityId,
            &PlayerUuid,
            &PlayerName,
            &Position,
            &Rotation,
        ),
        With<PlayerReady>,
    >,
) {
    let new_entity = event.entity;
    let game_mode = config.game_mode;

    let Ok((new_client_id, new_mc_id, new_uuid, new_name, new_pos, new_rot)) =
        new_player.get(new_entity)
    else {
        return;
    };

    tracing::info!(
        player_name = %new_name.0,
        player_uuid = %new_uuid.0,
        client_id = new_client_id.0,
        "Player connected"
    );

    // Send the new player their own tab list entry (no SpawnEntity for self)
    send_player_info(
        &channels,
        new_client_id.0,
        new_uuid.0,
        &new_name.0,
        game_mode,
    );

    for (other_client_id, other_mc_id, other_uuid, other_name, other_pos, other_rot) in
        all_players.iter()
    {
        if new_client_id.0 == other_client_id.0 {
            continue;
        }

        // Tell the new player about the existing player
        send_player_spawn(
            &channels,
            new_client_id.0,
            other_mc_id.0,
            other_uuid.0,
            &other_name.0,
            other_pos,
            other_rot,
            game_mode,
        );

        // Tell the existing player about the new player
        send_player_spawn(
            &channels,
            other_client_id.0,
            new_mc_id.0,
            new_uuid.0,
            &new_name.0,
            new_pos,
            new_rot,
            game_mode,
        );
    }
}

/// Observer: when a player quits, broadcast remove to all remaining ready players.
pub fn on_player_quit(
    event: On<PlayerQuitEvent>,
    channels: Res<NetworkChannels>,
    query: Query<(&MinecraftEntityId, &PlayerUuid, &PlayerName, &ClientId), With<PlayerReady>>,
    all_ready: Query<&ClientId, With<PlayerReady>>,
) {
    let disc_entity = event.entity;
    let disc_client_id = event.client_id;

    let Ok((mc_entity_id, player_uuid, player_name, _)) = query.get(disc_entity) else {
        return;
    };

    let eid = mc_entity_id.0;
    let uuid = player_uuid.0;

    for receiver_client_id in all_ready.iter() {
        if receiver_client_id.0 == disc_client_id {
            continue;
        }

        // RemoveEntities
        let _ = channels.outgoing.send(OutgoingPacket {
            client_id: receiver_client_id.0,
            packet: clientbound::ClientboundPacket::ManualPlay(
                clientbound::ManualPlayPacket::RemoveEntities(clientbound::RemoveEntities {
                    entity_ids: vec![eid],
                }),
            ),
        });

        // PlayerInfoRemove
        let _ = channels.outgoing.send(OutgoingPacket {
            client_id: receiver_client_id.0,
            packet: clientbound::ClientboundPacket::ManualPlay(
                clientbound::ManualPlayPacket::PlayerInfoRemove(clientbound::PlayerInfoRemove {
                    uuids: vec![uuid],
                }),
            ),
        });
    }

    tracing::info!(
        player_name = %player_name.0,
        player_uuid = %player_uuid.0,
        client_id = disc_client_id,
        "Player disconnected"
    );
}

/// Sends only PlayerInfoUpdate (tab list entry) without spawning the entity.
fn send_player_info(
    channels: &NetworkChannels,
    receiver_client_id: u32,
    uuid: uuid::Uuid,
    name: &str,
    game_mode: u8,
) {
    let _ = channels.outgoing.send(OutgoingPacket {
        client_id: receiver_client_id,
        packet: clientbound::ClientboundPacket::ManualPlay(
            clientbound::ManualPlayPacket::PlayerInfoUpdate(clientbound::PlayerInfoUpdate {
                entries: vec![clientbound::PlayerInfoEntry {
                    uuid,
                    name: name.to_string(),
                    game_mode: game_mode.into(),
                    listed: true,
                }],
            }),
        ),
    });
}

fn send_player_spawn(
    channels: &NetworkChannels,
    receiver_client_id: u32,
    entity_id: i32,
    uuid: uuid::Uuid,
    name: &str,
    pos: &Position,
    rot: &Rotation,
    game_mode: u8,
) {
    let yaw = (rot.yaw.rem_euclid(360.0) / 360.0 * 256.0) as u8;
    let pitch = (rot.pitch.rem_euclid(360.0) / 360.0 * 256.0) as u8;

    // Send PlayerInfoUpdate (adds to tab list)
    let _ = channels.outgoing.send(OutgoingPacket {
        client_id: receiver_client_id,
        packet: clientbound::ClientboundPacket::ManualPlay(
            clientbound::ManualPlayPacket::PlayerInfoUpdate(clientbound::PlayerInfoUpdate {
                entries: vec![clientbound::PlayerInfoEntry {
                    uuid,
                    name: name.to_string(),
                    game_mode: game_mode.into(),
                    listed: true,
                }],
            }),
        ),
    });

    // Send SpawnEntity (creates the entity in the world)
    let _ = channels.outgoing.send(OutgoingPacket {
        client_id: receiver_client_id,
        packet: clientbound::ClientboundPacket::Play(clientbound::PlayPacket::SpawnEntity(
            clientbound::SpawnEntity {
                entity_id,
                entity_uuid: uuid,
                entity_type: 155, // minecraft:player
                x: pos.x,
                y: pos.y,
                z: pos.z,
                velocity: voidmc_protocol::types::LpVec3::ZERO,
                pitch,
                yaw,
                head_yaw: yaw,
                data: 0,
            },
        )),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::IncomingPacket;

    #[test]
    fn player_spawn_wraps_negative_rotation() {
        let (incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, kick_rx) = flume::unbounded::<u32>();
        let channels = NetworkChannels {
            incoming: incoming_rx,
            outgoing: outgoing_tx,
            disconnect: disconnect_rx,
            kick: kick_tx,
        };

        send_player_spawn(
            &channels,
            7,
            42,
            uuid::Uuid::nil(),
            "player",
            &Position {
                x: 0.0,
                y: 64.0,
                z: 0.0,
            },
            &Rotation {
                yaw: -90.0,
                pitch: -45.0,
            },
            0,
        );

        let _player_info = outgoing_rx.recv().unwrap();
        let spawn = outgoing_rx.recv().unwrap();
        let clientbound::ClientboundPacket::Play(clientbound::PlayPacket::SpawnEntity(spawn)) =
            spawn.packet
        else {
            panic!("expected spawn entity packet");
        };

        assert_eq!(spawn.yaw, 192);
        assert_eq!(spawn.head_yaw, 192);
        assert_eq!(spawn.pitch, 224);

        drop((incoming_tx, disconnect_tx, kick_rx));
    }
}
