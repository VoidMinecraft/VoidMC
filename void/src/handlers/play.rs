use bevy_ecs::prelude::*;
use void_protocol::serverbound;

use crate::components::{
    ClientSettings, KeepAliveState, PlayerReady, Position, Rotation, TeleportState,
};
use crate::events::{PlayPacketEvent, PlayerMoveEvent, PlayerReadyEvent, PlayerRotateEvent};

pub fn handle_play_packet(
    world: &mut World,
    client_id: u32,
    entity: Entity,
    packet: serverbound::PlayPacket,
) {
    match &packet {
        serverbound::PlayPacket::ConfirmTeleportation(confirm) => {
            if let Some(mut teleport_state) = world.get_mut::<TeleportState>(entity) {
                if teleport_state.pending_id == Some(confirm.teleport_id) {
                    teleport_state.pending_id = None;
                    tracing::debug!(
                        "Client {} confirmed teleport {}",
                        client_id,
                        confirm.teleport_id
                    );
                } else {
                    tracing::warn!(
                        "Client {} confirmed unexpected teleport {} (expected {:?})",
                        client_id,
                        confirm.teleport_id,
                        teleport_state.pending_id
                    );
                }
            }
        }
        serverbound::PlayPacket::SetPlayerPos(pos) => {
            let old = world.get::<Position>(entity).cloned();
            if let Some(mut position) = world.get_mut::<Position>(entity) {
                position.x = pos.x;
                position.y = pos.y;
                position.z = pos.z;
            }
            if let Some(old) = old {
                world.trigger(PlayerMoveEvent {
                    entity,
                    old_x: old.x,
                    old_y: old.y,
                    old_z: old.z,
                    new_x: pos.x,
                    new_y: pos.y,
                    new_z: pos.z,
                });
                world.flush();
            }
        }
        serverbound::PlayPacket::SetPlayerPosAndRot(pos_rot) => {
            let old = world.get::<Position>(entity).cloned();
            if let Some(mut position) = world.get_mut::<Position>(entity) {
                position.x = pos_rot.x;
                position.y = pos_rot.y;
                position.z = pos_rot.z;
            }
            if let Some(mut rotation) = world.get_mut::<Rotation>(entity) {
                rotation.yaw = pos_rot.yaw;
                rotation.pitch = pos_rot.pitch;
            }
            if let Some(old) = old {
                world.trigger(PlayerMoveEvent {
                    entity,
                    old_x: old.x,
                    old_y: old.y,
                    old_z: old.z,
                    new_x: pos_rot.x,
                    new_y: pos_rot.y,
                    new_z: pos_rot.z,
                });
                world.flush();
            }
            world.trigger(PlayerRotateEvent {
                entity,
                yaw: pos_rot.yaw,
                pitch: pos_rot.pitch,
            });
            world.flush();
        }
        serverbound::PlayPacket::PlayerLoaded(_) => {
            tracing::info!("Client {} player loaded, marking ready", client_id);
            world.entity_mut(entity).insert(PlayerReady);
            world.trigger(PlayerReadyEvent { client_id, entity });
            world.flush();
        }
        serverbound::PlayPacket::TickEnd(_) => {
            // No-op
        }
        serverbound::PlayPacket::KeepAlive(ka) => {
            if let Some(mut keep_alive_state) = world.get_mut::<KeepAliveState>(entity) {
                if keep_alive_state.last_sent_id == ka.keep_alive_id {
                    keep_alive_state.awaiting_response = false;
                    tracing::debug!(
                        "Client {} responded to keep-alive {}",
                        client_id,
                        ka.keep_alive_id
                    );
                }
            }
        }
        serverbound::PlayPacket::SetPlayerRotation(rot) => {
            if let Some(mut rotation) = world.get_mut::<Rotation>(entity) {
                rotation.yaw = rot.yaw;
                rotation.pitch = rot.pitch;
            }
            world.trigger(PlayerRotateEvent {
                entity,
                yaw: rot.yaw,
                pitch: rot.pitch,
            });
            world.flush();
        }
        serverbound::PlayPacket::Pong(pong) => {
            tracing::debug!("Client {} pong: {}", client_id, pong.id);
        }
        serverbound::PlayPacket::ClientInformation(info) => {
            tracing::debug!(
                "Client {} updated settings (play): locale={}, view_distance={}",
                client_id,
                info.locale,
                info.view_distance
            );
            world.entity_mut(entity).insert(ClientSettings {
                locale: info.locale.clone(),
                view_distance: info.view_distance,
            });
        }
    }
    world.write_message(PlayPacketEvent {
        client_id,
        entity,
        packet,
    });
}
