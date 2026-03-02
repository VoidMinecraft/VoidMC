use bevy_ecs::prelude::*;
use void_protocol::serverbound;

use crate::components::{KeepAliveState, PlayerReady, Position, Rotation, TeleportState};
use crate::events::PlayPacketEvent;

pub fn handle_play(mut events: MessageReader<PlayPacketEvent>, mut commands: Commands, mut query: Query<(&mut Position, &mut Rotation, &mut TeleportState, &mut KeepAliveState)>) {
    for event in events.read() {
        match &event.packet {
            serverbound::PlayPacket::ConfirmTeleportation(confirm) => {
                if let Ok((_, _, mut teleport_state, _)) = query.get_mut(event.entity) {
                    if teleport_state.pending_id == Some(confirm.teleport_id) {
                        teleport_state.pending_id = None;
                        tracing::debug!(
                            "Client {} confirmed teleport {}",
                            event.client_id,
                            confirm.teleport_id
                        );
                    } else {
                        tracing::warn!(
                            "Client {} confirmed unexpected teleport {} (expected {:?})",
                            event.client_id,
                            confirm.teleport_id,
                            teleport_state.pending_id
                        );
                    }
                }
            }
            serverbound::PlayPacket::SetPlayerPos(pos) => {
                if let Ok((mut position, _, _, _)) = query.get_mut(event.entity) {
                    position.x = pos.x;
                    position.y = pos.y;
                    position.z = pos.z;
                }
            }
            serverbound::PlayPacket::SetPlayerPosAndRot(pos_rot) => {
                if let Ok((mut position, mut rotation, _, _)) = query.get_mut(event.entity) {
                    position.x = pos_rot.x;
                    position.y = pos_rot.y;
                    position.z = pos_rot.z;
                    rotation.yaw = pos_rot.yaw;
                    rotation.pitch = pos_rot.pitch;
                }
            }
            serverbound::PlayPacket::PlayerLoaded(_) => {
                tracing::info!("Client {} player loaded, marking ready", event.client_id);
                commands.entity(event.entity).insert(PlayerReady);
            }
            serverbound::PlayPacket::TickEnd(_) => {
                // No-op
            }
            serverbound::PlayPacket::KeepAlive(ka) => {
                if let Ok((_, _, _, mut keep_alive_state)) = query.get_mut(event.entity) {
                    if keep_alive_state.last_sent_id == ka.keep_alive_id {
                        keep_alive_state.awaiting_response = false;
                        tracing::debug!(
                            "Client {} responded to keep-alive {}",
                            event.client_id,
                            ka.keep_alive_id
                        );
                    }
                }
            }
            serverbound::PlayPacket::SetPlayerRotation(rot) => {
                if let Ok((_, mut rotation, _, _)) = query.get_mut(event.entity) {
                    rotation.yaw = rot.yaw;
                    rotation.pitch = rot.pitch;
                }
            }
            serverbound::PlayPacket::Pong(pong) => {
                tracing::debug!("Client {} pong: {}", event.client_id, pong.id);
            }
        }
    }
}
