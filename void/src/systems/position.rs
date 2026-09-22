use bevy_ecs::prelude::*;
use tracing::instrument;
use voidmc_protocol::clientbound;

use crate::components::{MinecraftEntityId, PlayerReady, Position, PreviousPosition, Rotation};
use crate::entity::Mount;
use crate::players::Players;
use crate::systems::entities::relative_delta;

#[instrument(level = "info", skip(players, moved_query, dismounted, resync_query))]
pub fn broadcast_position(
    players: Players,
    moved_query: Query<
        (
            Entity,
            &MinecraftEntityId,
            &Position,
            &PreviousPosition,
            Ref<Rotation>,
            Has<Mount>,
        ),
        (
            With<PlayerReady>,
            Or<(Changed<Position>, Changed<Rotation>)>,
        ),
    >,
    mut dismounted: RemovedComponents<Mount>,
    resync_query: Query<
        (&MinecraftEntityId, &Position, &Rotation),
        (With<PlayerReady>, Without<Mount>),
    >,
) {
    let ready = players.ready();
    let mut resynced = Vec::new();
    for rider in dismounted.read() {
        let Ok((mc_entity_id, pos, rotation)) = resync_query.get(rider) else {
            continue;
        };
        ready.send_except(rider, sync_packet(mc_entity_id.0, pos, rotation));
        ready.send_except(
            rider,
            clientbound::SetHeadRotation {
                entity_id: mc_entity_id.0,
                head_yaw: angle_to_byte(rotation.yaw),
            },
        );
        resynced.push(rider);
    }

    for (mover, mc_entity_id, pos, prev_pos, rotation, mounted) in moved_query.iter() {
        if resynced.contains(&mover) {
            continue;
        }

        let yaw = angle_to_byte(rotation.yaw);
        let pitch = angle_to_byte(rotation.pitch);

        if mounted {
            if !rotation.is_changed() {
                continue;
            }
            ready.send_except(
                mover,
                clientbound::UpdateEntityRotation {
                    entity_id: mc_entity_id.0,
                    yaw,
                    pitch,
                    on_ground: true,
                },
            );
            ready.send_except(
                mover,
                clientbound::SetHeadRotation {
                    entity_id: mc_entity_id.0,
                    head_yaw: yaw,
                },
            );
            continue;
        }

        let packet = if let (Some(delta_x), Some(delta_y), Some(delta_z)) = (
            relative_delta(pos.x, prev_pos.x),
            relative_delta(pos.y, prev_pos.y),
            relative_delta(pos.z, prev_pos.z),
        ) {
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
            )
        } else {
            sync_packet(mc_entity_id.0, pos, &rotation)
        };

        ready.send_except(mover, packet);
        ready.send_except(
            mover,
            clientbound::SetHeadRotation {
                entity_id: mc_entity_id.0,
                head_yaw: yaw,
            },
        );
    }
}

fn sync_packet(entity_id: i32, pos: &Position, rotation: &Rotation) -> clientbound::PlayPacket {
    clientbound::PlayPacket::EntityPositionSync(clientbound::EntityPositionSync {
        entity_id,
        x: pos.x,
        y: pos.y,
        z: pos.z,
        vx: 0.0,
        vy: 0.0,
        vz: 0.0,
        yaw: rotation.yaw,
        pitch: rotation.pitch,
        on_ground: true,
    })
}

fn angle_to_byte(angle: f32) -> u8 {
    (angle.rem_euclid(360.0) / 360.0 * 256.0) as u8
}

#[instrument(level = "info", skip(query))]
pub fn update_previous_positions(
    mut query: Query<
        (&Position, &mut PreviousPosition),
        (
            With<PlayerReady>,
            Or<(Changed<Position>, Changed<Rotation>)>,
        ),
    >,
) {
    for (pos, mut prev_pos) in query.iter_mut() {
        prev_pos.x = pos.x;
        prev_pos.y = pos.y;
        prev_pos.z = pos.z;
    }
}

#[cfg(test)]
mod tests {
    use bevy_app::{App, PostUpdate};

    use super::*;
    use crate::components::ClientId;
    use crate::network::{IncomingPacket, NetworkChannels, OutgoingPacket};

    #[test]
    fn broadcast_position_wraps_negative_rotation() {
        let (incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, kick_rx) = flume::unbounded::<u32>();
        let mut app = App::new();

        app.insert_resource(NetworkChannels {
            incoming: incoming_rx,
            outgoing: outgoing_tx,
            disconnect: disconnect_rx,
            kick: kick_tx,
        })
        .add_systems(PostUpdate, broadcast_position);

        app.world_mut().spawn((
            ClientId(1),
            MinecraftEntityId(42),
            Position {
                x: 0.0,
                y: 64.0,
                z: 0.0,
            },
            PreviousPosition {
                x: 0.0,
                y: 64.0,
                z: 0.0,
            },
            Rotation {
                yaw: -90.0,
                pitch: -45.0,
            },
            PlayerReady,
        ));
        app.world_mut().spawn((ClientId(2), PlayerReady));

        app.update();

        let rotation = outgoing_rx.recv().unwrap();
        let head_rotation = outgoing_rx.recv().unwrap();

        let clientbound::ClientboundPacket::Play(
            clientbound::PlayPacket::UpdateEntityPositionAndRotation(rotation),
        ) = rotation.packet
        else {
            panic!("expected position and rotation packet");
        };
        assert_eq!(rotation.yaw, 192);
        assert_eq!(rotation.pitch, 224);

        let clientbound::ClientboundPacket::Play(clientbound::PlayPacket::SetHeadRotation(
            head_rotation,
        )) = head_rotation.packet
        else {
            panic!("expected head rotation packet");
        };
        assert_eq!(head_rotation.head_yaw, 192);

        drop((incoming_tx, disconnect_tx, kick_rx));
    }

    #[test]
    fn broadcast_position_teleports_when_delta_overflows_i16() {
        let (incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, kick_rx) = flume::unbounded::<u32>();
        let mut app = App::new();

        app.insert_resource(NetworkChannels {
            incoming: incoming_rx,
            outgoing: outgoing_tx,
            disconnect: disconnect_rx,
            kick: kick_tx,
        })
        .add_systems(PostUpdate, broadcast_position);

        app.world_mut().spawn((
            ClientId(1),
            MinecraftEntityId(42),
            Position {
                x: 100.0,
                y: 64.0,
                z: 0.0,
            },
            PreviousPosition {
                x: 0.0,
                y: 64.0,
                z: 0.0,
            },
            Rotation {
                yaw: 0.0,
                pitch: 0.0,
            },
            PlayerReady,
        ));
        app.world_mut().spawn((ClientId(2), PlayerReady));

        app.update();

        let clientbound::ClientboundPacket::Play(clientbound::PlayPacket::EntityPositionSync(
            teleport,
        )) = outgoing_rx.recv().unwrap().packet
        else {
            panic!("expected absolute position sync for a move beyond the i16 delta range");
        };
        assert_eq!(teleport.entity_id, 42);
        assert_eq!((teleport.x, teleport.y, teleport.z), (100.0, 64.0, 0.0));

        drop((incoming_tx, disconnect_tx, kick_rx));
    }

    fn test_app() -> (App, flume::Receiver<OutgoingPacket>) {
        let (incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, kick_rx) = flume::unbounded::<u32>();
        let mut app = App::new();
        app.insert_resource(NetworkChannels {
            incoming: incoming_rx,
            outgoing: outgoing_tx,
            disconnect: disconnect_rx,
            kick: kick_tx,
        })
        .insert_non_send_resource((incoming_tx, disconnect_tx, kick_rx))
        .add_systems(
            PostUpdate,
            (broadcast_position, update_previous_positions).chain(),
        );
        (app, outgoing_rx)
    }

    fn packets(rx: &flume::Receiver<OutgoingPacket>) -> Vec<clientbound::PlayPacket> {
        rx.try_iter()
            .map(|out| match out.packet {
                clientbound::ClientboundPacket::Play(packet) => packet,
                other => panic!("unexpected packet {other:?}"),
            })
            .collect()
    }

    #[test]
    fn mounted_player_sends_rotation_only_and_teleports_on_dismount() {
        let (mut app, rx) = test_app();
        let vehicle = app.world_mut().spawn_empty().id();
        let rider = app
            .world_mut()
            .spawn((
                ClientId(1),
                MinecraftEntityId(42),
                Position {
                    x: 0.0,
                    y: 64.0,
                    z: 0.0,
                },
                PreviousPosition {
                    x: 0.0,
                    y: 64.0,
                    z: 0.0,
                },
                Rotation {
                    yaw: 0.0,
                    pitch: 0.0,
                },
                PlayerReady,
                Mount(vehicle),
            ))
            .id();
        app.world_mut().spawn((ClientId(2), PlayerReady));
        app.update();
        let _ = packets(&rx);

        let mut pos = app.world_mut().get_mut::<Position>(rider).unwrap();
        pos.x = 3.0;
        pos.z = -2.5;
        app.update();
        assert!(packets(&rx).is_empty());
        let prev = app.world().get::<PreviousPosition>(rider).unwrap();
        assert_eq!((prev.x, prev.y, prev.z), (3.0, 64.0, -2.5));

        let mut rotation = app.world_mut().get_mut::<Rotation>(rider).unwrap();
        rotation.yaw = -90.0;
        rotation.pitch = -45.0;
        app.update();
        let sent = packets(&rx);
        assert_eq!(sent.len(), 2);
        let clientbound::PlayPacket::UpdateEntityRotation(rot) = &sent[0] else {
            panic!("expected rotation-only packet, got {:?}", sent[0]);
        };
        assert_eq!(
            (rot.entity_id, rot.yaw, rot.pitch, rot.on_ground),
            (42, 192, 224, true)
        );
        let clientbound::PlayPacket::SetHeadRotation(head) = &sent[1] else {
            panic!("expected head rotation packet, got {:?}", sent[1]);
        };
        assert_eq!((head.entity_id, head.head_yaw), (42, 192));

        app.world_mut().entity_mut(rider).remove::<Mount>();
        app.update();
        let sent = packets(&rx);
        assert_eq!(sent.len(), 2);
        let clientbound::PlayPacket::EntityPositionSync(teleport) = &sent[0] else {
            panic!("expected absolute resync after dismount, got {:?}", sent[0]);
        };
        assert_eq!((teleport.x, teleport.y, teleport.z), (3.0, 64.0, -2.5));
        assert_eq!((teleport.yaw, teleport.pitch), (-90.0, -45.0));
        assert!(matches!(
            sent[1],
            clientbound::PlayPacket::SetHeadRotation(_)
        ));

        app.world_mut().get_mut::<Position>(rider).unwrap().x = 4.0;
        app.update();
        let sent = packets(&rx);
        assert_eq!(sent.len(), 2);
        let clientbound::PlayPacket::UpdateEntityPositionAndRotation(moved) = &sent[0] else {
            panic!("expected relative move after dismount, got {:?}", sent[0]);
        };
        assert_eq!((moved.delta_x, moved.delta_y, moved.delta_z), (4096, 0, 0));
    }

    #[test]
    fn dismounted_player_is_resynced_once_even_when_it_also_moved() {
        let (mut app, rx) = test_app();
        let vehicle = app.world_mut().spawn_empty().id();
        let rider = app
            .world_mut()
            .spawn((
                ClientId(1),
                MinecraftEntityId(7),
                Position {
                    x: 0.0,
                    y: 64.0,
                    z: 0.0,
                },
                PreviousPosition {
                    x: 0.0,
                    y: 64.0,
                    z: 0.0,
                },
                Rotation {
                    yaw: 0.0,
                    pitch: 0.0,
                },
                PlayerReady,
                Mount(vehicle),
            ))
            .id();
        app.world_mut().spawn((ClientId(2), PlayerReady));
        app.update();
        let _ = packets(&rx);

        app.world_mut().entity_mut(rider).remove::<Mount>();
        app.world_mut().get_mut::<Position>(rider).unwrap().y = 65.0;
        app.update();
        let sent = packets(&rx);
        assert_eq!(sent.len(), 2);
        let clientbound::PlayPacket::EntityPositionSync(teleport) = &sent[0] else {
            panic!("expected absolute resync after dismount, got {:?}", sent[0]);
        };
        assert_eq!(teleport.y, 65.0);
        let prev = app.world().get::<PreviousPosition>(rider).unwrap();
        assert_eq!(prev.y, 65.0);
    }
}
