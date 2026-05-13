use bevy_ecs::prelude::*;
use voidmc_protocol::clientbound;

use crate::components::{
    ClientId, EntityType, EntityUuid, MinecraftEntityId, PlayerName, PlayerReady, PlayerUuid,
    Position, Rotation, SpawnedEntity, Velocity,
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
    spawned_entities: Query<
        (
            &MinecraftEntityId,
            &EntityUuid,
            &Position,
            &Rotation,
            &Velocity,
            &EntityType,
        ),
        With<SpawnedEntity>,
    >,
) {
    let new_entity = event.entity;
    let game_mode = config.game_mode;

    let Ok((new_client_id, new_mc_id, new_uuid, new_name, new_pos, new_rot)) =
        new_player.get(new_entity)
    else {
        return;
    };

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

    // Send the new player all pre-existing summoned entities.
    for (mc_id, entity_uuid, pos, rot, vel, entity_type) in spawned_entities.iter() {
        let yaw = (rot.yaw / 360.0 * 256.0) as u8;
        let pitch = (rot.pitch / 360.0 * 256.0) as u8;
        let _ = channels.outgoing.send(OutgoingPacket {
            client_id: new_client_id.0,
            packet: voidmc_protocol::clientbound::ClientboundPacket::Play(
                voidmc_protocol::clientbound::PlayPacket::SpawnEntity(
                    voidmc_protocol::clientbound::SpawnEntity {
                        entity_id: mc_id.0,
                        entity_uuid: entity_uuid.0,
                        entity_type: entity_type.0,
                        x: pos.x,
                        y: pos.y,
                        z: pos.z,
                        pitch,
                        yaw,
                        head_yaw: yaw,
                        data: 0,
                        velocity: voidmc_protocol::types::LpVec3 {
                            x: vel.x as f64 / 8000.0,
                            y: vel.y as f64 / 8000.0,
                            z: vel.z as f64 / 8000.0,
                        },
                    },
                ),
            ),
        });
    }
}

/// Observer: when a player quits, broadcast remove to all remaining ready players.
pub fn on_player_quit(
    event: On<PlayerQuitEvent>,
    channels: Res<NetworkChannels>,
    query: Query<(&MinecraftEntityId, &PlayerUuid, &ClientId), With<PlayerReady>>,
    all_ready: Query<&ClientId, With<PlayerReady>>,
) {
    let disc_entity = event.entity;
    let disc_client_id = event.client_id;

    let Ok((mc_entity_id, player_uuid, _)) = query.get(disc_entity) else {
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
        "Player {} (entity {}) disconnected, notified remaining players",
        disc_client_id,
        eid
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
