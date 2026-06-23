use bevy_ecs::prelude::*;
use voidmc_protocol::clientbound;
use voidmc_protocol::types::LpVec3;

use crate::components::{
    ClientId, EntityDimension, EntityType, EntityUuid, MinecraftEntityId, PlayerDimension,
    PlayerReady, Position, PreviousPosition, Rotation, SpawnedEntity, Velocity,
};
use crate::events::{EntityDespawnEvent, PlayerReadyEvent};
use crate::network::{NetworkChannels, OutgoingPacket};

const RELATIVE_MOVE_SCALE: f64 = 4096.0;

pub fn on_player_ready_spawn_entities(
    event: On<PlayerReadyEvent>,
    channels: Res<NetworkChannels>,
    new_player: Query<(&ClientId, Option<&PlayerDimension>)>,
    spawned_entities: Query<
        (
            &MinecraftEntityId,
            &EntityUuid,
            &Position,
            &Rotation,
            &Velocity,
            &EntityType,
            Option<&EntityDimension>,
        ),
        With<SpawnedEntity>,
    >,
) {
    let Ok((client_id, player_dimension)) = new_player.get(event.entity) else {
        return;
    };

    for (entity_id, entity_uuid, position, rotation, velocity, entity_type, entity_dimension) in
        spawned_entities.iter()
    {
        if !is_visible_to(entity_dimension, player_dimension) {
            continue;
        }

        send_packet(
            &channels,
            client_id.0,
            spawn_entity_packet(
                entity_id.0,
                entity_uuid.0,
                entity_type.0,
                position,
                rotation,
                velocity,
            ),
        );
    }
}

pub fn broadcast_entity_spawns(
    channels: Res<NetworkChannels>,
    spawned_entities: Query<
        (
            &MinecraftEntityId,
            &EntityUuid,
            &Position,
            &Rotation,
            &Velocity,
            &EntityType,
            Option<&EntityDimension>,
        ),
        Added<SpawnedEntity>,
    >,
    ready_players: Query<(&ClientId, Option<&PlayerDimension>), With<PlayerReady>>,
) {
    for (entity_id, entity_uuid, position, rotation, velocity, entity_type, entity_dimension) in
        spawned_entities.iter()
    {
        let packet = spawn_entity_packet(
            entity_id.0,
            entity_uuid.0,
            entity_type.0,
            position,
            rotation,
            velocity,
        );

        for (client_id, player_dimension) in ready_players.iter() {
            if is_visible_to(entity_dimension, player_dimension) {
                send_packet(&channels, client_id.0, packet.clone());
            }
        }
    }
}

pub fn broadcast_entity_movement(
    channels: Res<NetworkChannels>,
    moved_entities: Query<
        (
            &MinecraftEntityId,
            Ref<Position>,
            &PreviousPosition,
            Ref<Rotation>,
            &Velocity,
            Option<&EntityDimension>,
        ),
        (
            With<SpawnedEntity>,
            Or<(Changed<Position>, Changed<Rotation>)>,
        ),
    >,
    ready_players: Query<(&ClientId, Option<&PlayerDimension>), With<PlayerReady>>,
) {
    for (entity_id, position, previous_position, rotation, velocity, entity_dimension) in
        moved_entities.iter()
    {
        if position.is_added() || rotation.is_added() {
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

        for (client_id, player_dimension) in ready_players.iter() {
            if !is_visible_to(entity_dimension, player_dimension) {
                continue;
            }

            send_packet(&channels, client_id.0, movement_packet.clone());
            if let Some(packet) = &head_rotation_packet {
                send_packet(&channels, client_id.0, packet.clone());
            }
        }
    }
}

pub fn broadcast_entity_motion(
    channels: Res<NetworkChannels>,
    moved_entities: Query<
        (&MinecraftEntityId, Ref<Velocity>, Option<&EntityDimension>),
        (With<SpawnedEntity>, Changed<Velocity>),
    >,
    ready_players: Query<(&ClientId, Option<&PlayerDimension>), With<PlayerReady>>,
) {
    for (entity_id, velocity, entity_dimension) in moved_entities.iter() {
        if velocity.is_added() {
            continue;
        }

        let packet = clientbound::ClientboundPacket::Play(
            clientbound::PlayPacket::SetEntityMotion(clientbound::SetEntityMotion {
                entity_id: entity_id.0,
                velocity: velocity_to_lp_vec3(&velocity),
            }),
        );

        for (client_id, player_dimension) in ready_players.iter() {
            if is_visible_to(entity_dimension, player_dimension) {
                send_packet(&channels, client_id.0, packet.clone());
            }
        }
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

pub fn on_entity_despawn(
    event: On<EntityDespawnEvent>,
    channels: Res<NetworkChannels>,
    mut commands: Commands,
    entities: Query<(&MinecraftEntityId, Option<&EntityDimension>), With<SpawnedEntity>>,
    ready_players: Query<(&ClientId, Option<&PlayerDimension>), With<PlayerReady>>,
) {
    let Ok((entity_id, entity_dimension)) = entities.get(event.entity) else {
        return;
    };

    let packet = clientbound::ClientboundPacket::ManualPlay(
        clientbound::ManualPlayPacket::RemoveEntities(clientbound::RemoveEntities {
            entity_ids: vec![entity_id.0],
        }),
    );

    for (client_id, player_dimension) in ready_players.iter() {
        if is_visible_to(entity_dimension, player_dimension) {
            send_packet(&channels, client_id.0, packet.clone());
        }
    }

    commands.entity(event.entity).despawn();
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
                            on_ground: true,
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
                    on_ground: true,
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
                on_ground: true,
            },
        ));
    }

    clientbound::ClientboundPacket::Play(clientbound::PlayPacket::UpdateEntityRotation(
        clientbound::UpdateEntityRotation {
            entity_id,
            yaw,
            pitch,
            on_ground: true,
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

fn relative_delta(current: f64, previous: f64) -> Option<i16> {
    let delta = ((current - previous) * RELATIVE_MOVE_SCALE).round();
    if delta < i16::MIN as f64 || delta > i16::MAX as f64 {
        None
    } else {
        Some(delta as i16)
    }
}

fn angle_to_byte(angle: f32) -> u8 {
    (angle.rem_euclid(360.0) / 360.0 * 256.0) as u8
}

fn is_visible_to(
    entity_dimension: Option<&EntityDimension>,
    player_dimension: Option<&PlayerDimension>,
) -> bool {
    match entity_dimension {
        Some(entity_dimension) => player_dimension
            .map(|player_dimension| player_dimension.0 == entity_dimension.0)
            .unwrap_or(false),
        None => true,
    }
}

fn send_packet(channels: &NetworkChannels, client_id: u32, packet: clientbound::ClientboundPacket) {
    let _ = channels.outgoing.send(OutgoingPacket { client_id, packet });
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
