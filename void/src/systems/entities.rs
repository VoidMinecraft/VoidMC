use bevy_ecs::prelude::*;
use tracing::instrument;
use voidmc_protocol::clientbound;
use voidmc_protocol::types::LpVec3;

use crate::components::{
    EntityViewers, Grounded, MinecraftEntityId, Position, PreviousPosition, Rotation,
    SpawnedEntity, Velocity,
};
use crate::players::Players;

const RELATIVE_MOVE_SCALE: f64 = 4096.0;

#[instrument(
    name = "entity_movement_broadcast",
    level = "info",
    skip(players, moved_entities)
)]
pub fn broadcast_entity_movement(
    players: Players,
    moved_entities: Query<
        (
            &MinecraftEntityId,
            Ref<Position>,
            &PreviousPosition,
            Ref<Rotation>,
            &Velocity,
            Option<&Grounded>,
            &EntityViewers,
        ),
        (
            With<SpawnedEntity>,
            Or<(Changed<Position>, Changed<Rotation>)>,
        ),
    >,
) {
    for (entity_id, position, previous_position, rotation, velocity, grounded, viewers) in
        moved_entities.iter()
    {
        if position.is_added() || rotation.is_added() || viewers.is_empty() {
            continue;
        }

        let position_changed = position.is_changed();
        let rotation_changed = rotation.is_changed();
        if !position_changed && !rotation_changed {
            continue;
        }

        let movement_packet = movement_packet(
            entity_id.0,
            &position,
            previous_position,
            &rotation,
            velocity,
            grounded.map(|grounded| grounded.0).unwrap_or(true),
            position_changed,
            rotation_changed,
        );
        let head_rotation_packet = rotation_changed.then(|| {
            clientbound::ClientboundPacket::Play(clientbound::PlayPacket::SetHeadRotation(
                clientbound::SetHeadRotation {
                    entity_id: entity_id.0,
                    head_yaw: angle_to_byte(rotation.yaw),
                },
            ))
        });

        players.send_to(viewers.iter(), movement_packet);
        if let Some(packet) = head_rotation_packet {
            players.send_to(viewers.iter(), packet);
        }
    }
}

#[instrument(
    name = "entity_motion_broadcast",
    level = "info",
    skip(players, moved_entities)
)]
pub fn broadcast_entity_motion(
    players: Players,
    moved_entities: Query<
        (&MinecraftEntityId, Ref<Velocity>, &EntityViewers),
        (With<SpawnedEntity>, Changed<Velocity>),
    >,
) {
    for (entity_id, velocity, viewers) in moved_entities.iter() {
        if velocity.is_added() || viewers.is_empty() {
            continue;
        }

        let packet = clientbound::SetEntityMotion {
            entity_id: entity_id.0,
            velocity: velocity_to_lp_vec3(&velocity),
        };

        players.send_to(viewers.iter(), packet);
    }
}

pub fn update_previous_entity_positions(
    mut query: Query<(&Position, &mut PreviousPosition), (With<SpawnedEntity>, Changed<Position>)>,
) {
    for (position, mut previous_position) in query.iter_mut() {
        previous_position.x = position.x;
        previous_position.y = position.y;
        previous_position.z = position.z;
    }
}

pub fn spawn_entity_packet(
    entity_id: i32,
    entity_uuid: uuid::Uuid,
    entity_type: i32,
    position: &Position,
    rotation: &Rotation,
    velocity: &Velocity,
) -> clientbound::ClientboundPacket {
    let yaw = angle_to_byte(rotation.yaw);
    let pitch = angle_to_byte(rotation.pitch);

    clientbound::ClientboundPacket::Play(clientbound::PlayPacket::SpawnEntity(
        clientbound::SpawnEntity {
            entity_id,
            entity_uuid,
            entity_type,
            x: position.x,
            y: position.y,
            z: position.z,
            velocity: velocity_to_lp_vec3(velocity),
            pitch,
            yaw,
            head_yaw: yaw,
            data: 0,
        },
    ))
}

fn movement_packet(
    entity_id: i32,
    position: &Position,
    previous_position: &PreviousPosition,
    rotation: &Rotation,
    velocity: &Velocity,
    on_ground: bool,
    position_changed: bool,
    rotation_changed: bool,
) -> clientbound::ClientboundPacket {
    let yaw = angle_to_byte(rotation.yaw);
    let pitch = angle_to_byte(rotation.pitch);

    if position_changed {
        let delta_x = relative_delta(position.x, previous_position.x);
        let delta_y = relative_delta(position.y, previous_position.y);
        let delta_z = relative_delta(position.z, previous_position.z);

        if let (Some(delta_x), Some(delta_y), Some(delta_z)) = (delta_x, delta_y, delta_z) {
            if rotation_changed {
                return clientbound::ClientboundPacket::Play(
                    clientbound::PlayPacket::UpdateEntityPositionAndRotation(
                        clientbound::UpdateEntityPositionAndRotation {
                            entity_id,
                            delta_x,
                            delta_y,
                            delta_z,
                            yaw,
                            pitch,
                            on_ground,
                        },
                    ),
                );
            }

            return clientbound::ClientboundPacket::Play(
                clientbound::PlayPacket::UpdateEntityPosition(clientbound::UpdateEntityPosition {
                    entity_id,
                    delta_x,
                    delta_y,
                    delta_z,
                    on_ground,
                }),
            );
        }

        return clientbound::ClientboundPacket::Play(clientbound::PlayPacket::TeleportEntity(
            clientbound::TeleportEntity {
                entity_id,
                x: position.x,
                y: position.y,
                z: position.z,
                vx: velocity.x,
                vy: velocity.y,
                vz: velocity.z,
                yaw: rotation.yaw,
                pitch: rotation.pitch,
                relatives: clientbound::TeleportFlags::empty(),
                on_ground,
            },
        ));
    }

    clientbound::ClientboundPacket::Play(clientbound::PlayPacket::UpdateEntityRotation(
        clientbound::UpdateEntityRotation {
            entity_id,
            yaw,
            pitch,
            on_ground,
        },
    ))
}

fn velocity_to_lp_vec3(velocity: &Velocity) -> LpVec3 {
    LpVec3 {
        x: velocity.x,
        y: velocity.y,
        z: velocity.z,
    }
}

pub(crate) fn relative_delta(current: f64, previous: f64) -> Option<i16> {
    let delta = (current * RELATIVE_MOVE_SCALE).round() - (previous * RELATIVE_MOVE_SCALE).round();
    if delta < i16::MIN as f64 || delta > i16::MAX as f64 {
        None
    } else {
        Some(delta as i16)
    }
}

fn angle_to_byte(angle: f32) -> u8 {
    (angle.rem_euclid(360.0) / 360.0 * 256.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_delta_uses_protocol_scale() {
        assert_eq!(relative_delta(1.25, 1.0), Some(1024));
        assert_eq!(relative_delta(-1.0, 1.0), Some(-8192));
    }

    #[test]
    fn relative_delta_matches_vec_delta_codec_rounding() {
        let previous = 10.0;
        let current = previous + 0.3;
        let expected = (current * 4096.0_f64).round() - (previous * 4096.0_f64).round();
        assert_eq!(relative_delta(current, previous), Some(expected as i16));
        assert_eq!(relative_delta(0.000_1, 0.0), Some(0));
        assert_eq!(relative_delta(0.000_2, 0.000_1), Some(1));
    }

    #[test]
    fn relative_delta_accumulates_without_drift() {
        let start = 100.25;
        let mut previous = start;
        let mut client_fixed = (start * 4096.0_f64).round() as i64;
        for tick in 1..=300 {
            let current = start + 0.3 * tick as f64;
            let delta = relative_delta(current, previous).expect("delta within i16 range");
            client_fixed += i64::from(delta);
            previous = current;
        }
        let final_position = start + 0.3 * 300.0;
        assert_eq!(client_fixed, (final_position * 4096.0_f64).round() as i64);
        assert_eq!(
            client_fixed - (start * 4096.0_f64).round() as i64,
            (final_position * 4096.0_f64).round() as i64 - (start * 4096.0_f64).round() as i64
        );
    }

    #[test]
    fn movement_packet_encodes_fixed_point_deltas() {
        use voidmc_codec::Encode;

        let packet = movement_packet(
            7,
            &Position {
                x: 0.4,
                y: 64.0,
                z: -3.8,
            },
            &PreviousPosition {
                x: 0.1,
                y: 64.0,
                z: -3.5,
            },
            &Rotation {
                yaw: 0.0,
                pitch: 0.0,
            },
            &Velocity {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            true,
            true,
            false,
        );

        let clientbound::ClientboundPacket::Play(play_packet) = packet else {
            panic!("expected Play packet");
        };
        let mut bytes = Vec::new();
        play_packet.encode(&mut bytes);
        assert_eq!(
            bytes,
            [0x35, 0x07, 0x04, 0xCC, 0x00, 0x00, 0xFB, 0x33, 0x01]
        );
    }

    #[test]
    fn relative_delta_rejects_large_moves() {
        assert_eq!(relative_delta(9.0, 0.0), None);
        assert_eq!(relative_delta(-9.0, 0.0), None);
    }

    #[test]
    fn angles_wrap_to_protocol_byte() {
        assert_eq!(angle_to_byte(0.0), 0);
        assert_eq!(angle_to_byte(90.0), 64);
        assert_eq!(angle_to_byte(360.0), 0);
        assert_eq!(angle_to_byte(-90.0), 192);
    }

    #[test]
    fn spawn_packet_uses_velocity_directly() {
        let packet = spawn_entity_packet(
            42,
            uuid::Uuid::nil(),
            150,
            &Position {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
            &Rotation {
                yaw: 90.0,
                pitch: 45.0,
            },
            &Velocity {
                x: 0.5,
                y: 0.25,
                z: -0.5,
            },
        );

        let clientbound::ClientboundPacket::Play(clientbound::PlayPacket::SpawnEntity(packet)) =
            packet
        else {
            panic!("expected SpawnEntity packet");
        };

        assert_eq!(packet.velocity.x, 0.5);
        assert_eq!(packet.velocity.y, 0.25);
        assert_eq!(packet.velocity.z, -0.5);
        assert_eq!(packet.yaw, 64);
        assert_eq!(packet.pitch, 32);
    }
}
