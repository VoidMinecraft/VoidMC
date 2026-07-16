use bevy_ecs::prelude::*;
use tracing::instrument;
use voidmc_protocol::clientbound;

use crate::components::{
    ClientId, MinecraftEntityId, PlayerReady, Position, PreviousPosition, Rotation,
};
use crate::network::{NetworkChannels, OutgoingPacket};

#[instrument(level = "info", skip(channels, moved_query, all_players))]
pub fn broadcast_position(
    channels: Res<NetworkChannels>,
    moved_query: Query<
        (
            &ClientId,
            &MinecraftEntityId,
            &Position,
            &PreviousPosition,
            &Rotation,
        ),
        (
            With<PlayerReady>,
            Or<(Changed<Position>, Changed<Rotation>)>,
        ),
    >,
    all_players: Query<&ClientId, With<PlayerReady>>,
) {
    for (sender_client_id, mc_entity_id, pos, prev_pos, rotation) in moved_query.iter() {
        let delta_x = ((pos.x * 32.0 - prev_pos.x * 32.0) * 128.0) as i16;
        let delta_y = ((pos.y * 32.0 - prev_pos.y * 32.0) * 128.0) as i16;
        let delta_z = ((pos.z * 32.0 - prev_pos.z * 32.0) * 128.0) as i16;

        let yaw = (rotation.yaw.rem_euclid(360.0) / 360.0 * 256.0) as u8;
        let pitch = (rotation.pitch.rem_euclid(360.0) / 360.0 * 256.0) as u8;

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
    use crate::network::{IncomingPacket, NetworkChannels};

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
}
