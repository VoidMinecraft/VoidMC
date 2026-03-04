use bevy_ecs::prelude::*;
use void_protocol::serverbound;

use crate::commands::{self, CommandRegistry};
use crate::components::{
    ClientId, ClientSettings, KeepAliveState, PlayerName, PlayerReady, Position, Rotation,
    TeleportState,
};
use crate::events::{
    ChatCommandEvent, ChatMessageEvent, PlayPacketEvent, PlayerMoveEvent, PlayerReadyEvent,
    PlayerRotateEvent,
};
use crate::network::{NetworkChannels, OutgoingPacket};

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
        serverbound::PlayPacket::ChatCommand(cmd) => {
            handle_chat_command(world, client_id, entity, &cmd.command);
        }
        serverbound::PlayPacket::ChatCommandUnsigned(cmd) => {
            handle_chat_command(world, client_id, entity, &cmd.command);
        }
        serverbound::PlayPacket::SignedChatCommand(cmd) => {
            handle_chat_command(world, client_id, entity, &cmd.command);
        }
        serverbound::PlayPacket::ChatMessage(msg) => {
            handle_chat_message(world, client_id, entity, &msg.message);
        }
        serverbound::PlayPacket::CommandSuggestionsRequest(req) => {
            handle_command_suggestions(world, client_id, &req.text, req.transaction_id);
        }
    }
    world.write_message(PlayPacketEvent {
        client_id,
        entity,
        packet,
    });
}

fn handle_chat_command(world: &mut World, client_id: u32, entity: Entity, raw_command: &str) {
    let parts: Vec<String> = raw_command.split_whitespace().map(String::from).collect();
    let (command_name, args) = match parts.split_first() {
        Some((name, rest)) => (name.clone(), rest.to_vec()),
        None => return,
    };

    tracing::info!("Client {} executed command: /{}", client_id, raw_command);

    // Dispatch via Arc-cloned handler — registry stays in the World the whole time.
    commands::dispatch_command(world, client_id, entity, &command_name, args.clone());

    // Trigger semantic event for external observers
    world.trigger(ChatCommandEvent {
        entity,
        client_id,
        command: command_name,
        args,
    });
    world.flush();
}

fn handle_chat_message(world: &mut World, client_id: u32, entity: Entity, message: &str) {
    // If the client doesn't recognise a command in its tree, it sends
    // "/command args" as a ChatMessage instead of ChatCommand.  Intercept that.
    if let Some(cmd) = message.strip_prefix('/') {
        handle_chat_command(world, client_id, entity, cmd);
        return;
    }

    let player_name = world
        .get::<PlayerName>(entity)
        .map(|n| n.0.clone())
        .unwrap_or_else(|| "Unknown".to_string());

    tracing::info!("<{}> {}", player_name, message);

    // Broadcast the chat message to all ready players
    let formatted = format!("<{}> {}", player_name, message);
    let nbt = crate::commands::text_to_nbt(&formatted, "white");
    let packet = void_protocol::clientbound::ClientboundPacket::Play(
        void_protocol::clientbound::PlayPacket::SystemChat(
            void_protocol::clientbound::SystemChat {
                content: nbt,
                overlay: false,
            },
        ),
    );

    let channels = world.resource::<NetworkChannels>();
    let sender = channels.outgoing.clone();
    let ready_clients: Vec<u32> = world
        .query_filtered::<&ClientId, With<PlayerReady>>()
        .iter(world)
        .map(|c| c.0)
        .collect();

    for cid in ready_clients {
        let _ = sender.send(OutgoingPacket {
            client_id: cid,
            packet: packet.clone(),
        });
    }

    // Trigger semantic event
    world.trigger(ChatMessageEvent {
        entity,
        client_id,
        message: message.to_string(),
    });
    world.flush();
}

fn handle_command_suggestions(
    world: &mut World,
    client_id: u32,
    text: &str,
    transaction_id: i32,
) {
    // text is e.g. "/kick dan" — split into command + partial arg
    let without_slash = text.strip_prefix('/').unwrap_or(text);
    let parts: Vec<&str> = without_slash.splitn(2, ' ').collect();
    let command_name = parts[0];

    // Verify the command exists
    let exists = world.resource::<CommandRegistry>().resolve(command_name).is_some();
    if !exists {
        return;
    }

    // Extract partial token being typed
    let arg_text = parts.get(1).copied().unwrap_or("");
    let completing_new = text.ends_with(' ');
    let partial = if completing_new || arg_text.is_empty() {
        ""
    } else {
        arg_text.split_whitespace().last().unwrap_or("")
    };

    // Collect online player names matching the partial input
    let names: Vec<String> = world
        .query_filtered::<&PlayerName, With<PlayerReady>>()
        .iter(world)
        .map(|n| n.0.clone())
        .filter(|name| name.to_lowercase().starts_with(&partial.to_lowercase()))
        .collect();

    // Calculate start position: position of the partial token in the original text
    let start = if partial.is_empty() {
        text.len() as i32
    } else {
        (text.len() - partial.len()) as i32
    };

    let response = void_protocol::clientbound::CommandSuggestionsResponse {
        transaction_id,
        start,
        length: partial.len() as i32,
        matches: names,
    };

    let channels = world.resource::<NetworkChannels>();
    let _ = channels.outgoing.send(OutgoingPacket {
        client_id,
        packet: void_protocol::clientbound::ClientboundPacket::ManualPlay(
            void_protocol::clientbound::ManualPlayPacket::CommandSuggestionsResponse(response),
        ),
    });
}
