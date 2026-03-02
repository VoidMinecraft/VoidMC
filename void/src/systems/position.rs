use bevy_ecs::prelude::*;
use void_protocol::clientbound;

use crate::components::{
    ClientId, MinecraftEntityId, PlayerReady, Position, PreviousPosition, Rotation,
};
use crate::network::{NetworkChannels, OutgoingPacket};

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
