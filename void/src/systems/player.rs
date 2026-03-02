use bevy_ecs::prelude::*;
use void_protocol::clientbound;

use crate::components::{
    ClientId, MinecraftEntityId, PlayerName, PlayerReady, PlayerUuid, Position, Rotation,
};
use crate::network::{ClientToEntityMap, NetworkChannels, OutgoingPacket};

/// Sends PlayerInfoUpdate + SpawnEntity to introduce new players to existing players and vice versa.
pub fn spawn_players_for_new_connections(
    channels: Res<NetworkChannels>,
    new_players: Query<
        (
            &ClientId,
            &MinecraftEntityId,
            &PlayerUuid,
            &PlayerName,
            &Position,
            &Rotation,
        ),
        Added<PlayerReady>,
    >,
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
    for (new_client_id, new_mc_id, new_uuid, new_name, new_pos, new_rot) in new_players.iter() {
        // Send the new player their own tab list entry (no SpawnEntity for self)
        send_player_info(
            &channels,
            new_client_id.0,
            new_uuid.0,
            &new_name.0,
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
            );
        }
    }
}

/// Sends only PlayerInfoUpdate (tab list entry) without spawning the entity.
fn send_player_info(
    channels: &NetworkChannels,
    receiver_client_id: u32,
    uuid: uuid::Uuid,
    name: &str,
) {
    let _ = channels.outgoing.send(OutgoingPacket {
        client_id: receiver_client_id,
        packet: clientbound::ClientboundPacket::ManualPlay(
            clientbound::ManualPlayPacket::PlayerInfoUpdate(clientbound::PlayerInfoUpdate {
                entries: vec![clientbound::PlayerInfoEntry {
                    uuid,
                    name: name.to_string(),
                    game_mode: 1, // Creative
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
) {
    let yaw = (rot.yaw / 360.0 * 256.0) as u8;
    let pitch = (rot.pitch / 360.0 * 256.0) as u8;

    // Send PlayerInfoUpdate (adds to tab list)
    let _ = channels.outgoing.send(OutgoingPacket {
        client_id: receiver_client_id,
        packet: clientbound::ClientboundPacket::ManualPlay(
            clientbound::ManualPlayPacket::PlayerInfoUpdate(clientbound::PlayerInfoUpdate {
                entries: vec![clientbound::PlayerInfoEntry {
                    uuid,
                    name: name.to_string(),
                    game_mode: 1, // Creative
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
                entity_type: 147, // Player
                x: pos.x,
                y: pos.y,
                z: pos.z,
                pitch,
                yaw,
                head_yaw: yaw,
                data: 0,
                velocity_x: 0,
                velocity_y: 0,
                velocity_z: 0,
            },
        )),
    });
}

/// Drains the disconnect channel and cleans up disconnected players.
pub fn handle_disconnects(
    channels: Res<NetworkChannels>,
    mut client_to_entity: ResMut<ClientToEntityMap>,
    query: Query<(&MinecraftEntityId, &PlayerUuid, &ClientId), With<PlayerReady>>,
    all_ready: Query<&ClientId, With<PlayerReady>>,
    mut commands: Commands,
) {
    let mut disconnected: Vec<u32> = Vec::new();
    while let Ok(client_id) = channels.disconnect.try_recv() {
        disconnected.push(client_id);
    }

    for disc_client_id in disconnected {
        let entity = match client_to_entity.0.remove(&disc_client_id) {
            Some(e) => e,
            None => continue,
        };

        // Check if this entity was a ready player with the needed components
        if let Ok((mc_entity_id, player_uuid, _)) = query.get(entity) {
            let eid = mc_entity_id.0;
            let uuid = player_uuid.0;

            // Notify all remaining players
            for receiver_client_id in all_ready.iter() {
                if receiver_client_id.0 == disc_client_id {
                    continue;
                }

                // RemoveEntities
                let _ = channels.outgoing.send(OutgoingPacket {
                    client_id: receiver_client_id.0,
                    packet: clientbound::ClientboundPacket::ManualPlay(
                        clientbound::ManualPlayPacket::RemoveEntities(
                            clientbound::RemoveEntities {
                                entity_ids: vec![eid],
                            },
                        ),
                    ),
                });

                // PlayerInfoRemove
                let _ = channels.outgoing.send(OutgoingPacket {
                    client_id: receiver_client_id.0,
                    packet: clientbound::ClientboundPacket::ManualPlay(
                        clientbound::ManualPlayPacket::PlayerInfoRemove(
                            clientbound::PlayerInfoRemove {
                                uuids: vec![uuid],
                            },
                        ),
                    ),
                });
            }

            tracing::info!(
                "Player {} (entity {}) disconnected, notified remaining players",
                disc_client_id,
                eid
            );
        }

        commands.entity(entity).despawn();
    }
}
