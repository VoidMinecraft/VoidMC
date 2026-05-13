use bevy_ecs::prelude::*;
use tracing::instrument;
use voidmc_protocol::clientbound;

use crate::components::{
    ClientId, MinecraftEntityId, PlayerReady, Position, PreviousPosition, Rotation,
    SpawnedEntity, MovementUpdateCooldown,
    Grounded,
};
use crate::network::{NetworkChannels, OutgoingPacket};

#[instrument(level = "info", skip(channels, moved_query, all_players))]
pub fn broadcast_player_position(
    channels: Res<NetworkChannels>,
    moved_query: Query<(
        &ClientId,
        &MinecraftEntityId,
        &Position,
        &PreviousPosition,
        &Rotation,
    ), (
        With<PlayerReady>,
        Or<(Changed<Position>, Changed<Rotation>)>,
    )>,
    all_players: Query<&ClientId, With<PlayerReady>>,
) {
    for (sender_client_id, mc_entity_id, pos, prev_pos, rotation) in moved_query.iter() {
        let delta_x = ((pos.x * 32.0 - prev_pos.x * 32.0) * 128.0) as i16;
        let delta_y = ((pos.y * 32.0 - prev_pos.y * 32.0) * 128.0) as i16;
        let delta_z = ((pos.z * 32.0 - prev_pos.z * 32.0) * 128.0) as i16;

        let yaw = (rotation.yaw / 360.0 * 256.0) as u8;
        let pitch = (rotation.pitch / 360.0 * 256.0) as u8;

        for receiver_client_id in all_players.iter() {
            if receiver_client_id.0 == sender_client_id.0 {
                continue;
            }

            // Send position + rotation update
            let _ = channels.outgoing.send(OutgoingPacket {
                client_id: receiver_client_id.0,
                packet: clientbound::ClientboundPacket::Play(
                    clientbound::PlayPacket::UpdateEntityPositionAndRotation(
                        clientbound::UpdateEntityPositionAndRotation {
                            entity_id: mc_entity_id.0,
                            delta_x,
                            delta_y,
                            delta_z,
                            yaw,
                            pitch,
                            on_ground: true,
                        },
                    ),
                ),
            });

            // Send head rotation
            let _ = channels.outgoing.send(OutgoingPacket {
                client_id: receiver_client_id.0,
                packet: clientbound::ClientboundPacket::Play(
                    clientbound::PlayPacket::SetHeadRotation(clientbound::SetHeadRotation {
                        entity_id: mc_entity_id.0,
                        head_yaw: yaw,
                    }),
                ),
            });
        }
    }
}

pub fn broadcast_spawned_position(
    channels: Res<NetworkChannels>,
    mut spawned_query: Query<(
        &MinecraftEntityId,
        &Position,
        &PreviousPosition,
        &Rotation,
        &Grounded,
        &mut MovementUpdateCooldown,
    ), (With<SpawnedEntity>, Or<(Changed<Position>, Changed<Rotation>, Changed<crate::components::VerticalVelocity>)>)>,
    all_players: Query<&ClientId, With<PlayerReady>>,
) {
    const THROTTLE_TICKS: u8 = 2;

    for (mc_entity_id, pos, prev_pos, rotation, grounded, mut cooldown) in spawned_query.iter_mut() {
        if cooldown.0 != 0 {
            continue;
        }

        send_position_update(&channels, &all_players, mc_entity_id, pos, prev_pos, rotation, *grounded);
        cooldown.0 = THROTTLE_TICKS;
    }
}

fn send_position_update(
    channels: &Res<NetworkChannels>,
    all_players: &Query<&ClientId, With<PlayerReady>>,
    mc_entity_id: &MinecraftEntityId,
    pos: &Position,
    prev_pos: &PreviousPosition,
    rotation: &Rotation,
    grounded: Grounded,
) {
    let delta_x = ((pos.x * 32.0 - prev_pos.x * 32.0) * 128.0) as i16;
    let delta_y = ((pos.y * 32.0 - prev_pos.y * 32.0) * 128.0) as i16;
    let delta_z = ((pos.z * 32.0 - prev_pos.z * 32.0) * 128.0) as i16;

    let yaw = (rotation.yaw / 360.0 * 256.0) as u8;
    let pitch = (rotation.pitch / 360.0 * 256.0) as u8;

    for receiver_client_id in all_players.iter() {
        let _ = channels.outgoing.send(OutgoingPacket {
            client_id: receiver_client_id.0,
            packet: clientbound::ClientboundPacket::Play(
                clientbound::PlayPacket::UpdateEntityPositionAndRotation(
                    clientbound::UpdateEntityPositionAndRotation {
                        entity_id: mc_entity_id.0,
                        delta_x,
                        delta_y,
                        delta_z,
                        yaw,
                        pitch,
                        on_ground: grounded.0,
                    },
                ),
            ),
        });

        let _ = channels.outgoing.send(OutgoingPacket {
            client_id: receiver_client_id.0,
            packet: clientbound::ClientboundPacket::Play(
                clientbound::PlayPacket::SetHeadRotation(clientbound::SetHeadRotation {
                    entity_id: mc_entity_id.0,
                    head_yaw: yaw,
                }),
            ),
        });
    }
}

#[instrument(level = "info", skip(query))]
pub fn update_previous_player_positions(
    mut query: Query<(&Position, &mut PreviousPosition), (With<PlayerReady>, Or<(Changed<Position>, Changed<Rotation>)>)>,
) {
    for (pos, mut prev_pos) in query.iter_mut() {
        prev_pos.x = pos.x;
        prev_pos.y = pos.y;
        prev_pos.z = pos.z;
    }
}


pub fn update_previous_spawned_positions(
    mut query: Query<(&Position, &mut PreviousPosition), (With<SpawnedEntity>, Or<(Changed<Position>, Changed<Rotation>)>)>,
) {
    for (pos, mut prev_pos) in query.iter_mut() {
        prev_pos.x = pos.x;
        prev_pos.y = pos.y;
        prev_pos.z = pos.z;
    }
}

pub fn force_updates_for_recently_spawned(
    mut query: Query<&mut MovementUpdateCooldown, (With<SpawnedEntity>, With<crate::components::RecentlySpawned>)>,
) {
    for mut cooldown in query.iter_mut() {
        cooldown.0 = 0;
    }
}

pub fn decrement_movement_cooldowns(mut query: Query<&mut MovementUpdateCooldown, With<SpawnedEntity>>) {
    for mut cooldown in query.iter_mut() {
        if cooldown.0 > 0 {
            cooldown.0 -= 1;
        }
    }
}
