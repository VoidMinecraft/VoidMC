use bevy_ecs::prelude::*;
use void_codec::Decode;
use void_protocol::serverbound;
use void_protocol::types::{Hand, PlayerActionStatus, PlayerCommandAction};

use crate::commands::{self, CommandRegistry};
use crate::components::{ClientId, ClientSettings, KeepAliveState, PlayerName, PlayerReady};
use crate::events::{
    ChatCommandEvent, ChatMessageEvent, PlayerCancelDiggingEvent, PlayerChangeSlotEvent,
    PlayerCloseContainerEvent, PlayerDropItemEvent, PlayerFinishDiggingEvent,
    PlayerInteractEntityEvent, PlayerReadyEvent, PlayerSneakEvent, PlayerSprintEvent,
    PlayerStartDiggingEvent, PlayerSwapHandsEvent, PlayerSwingArmEvent, PlayerToggleFlyEvent,
    PlayerUseItemEvent, PlayerUseItemOnBlockEvent,
};
use crate::network::{NetworkChannels, OutgoingPacket};

pub fn handle_play_packet(
    world: &mut World,
    client_id: u32,
    entity: Entity,
    packet: serverbound::PlayPacket,
) {
    match &packet {
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
        serverbound::PlayPacket::SwingArm(p) => {
            world.trigger(PlayerSwingArmEvent {
                entity,
                hand: p.hand,
            });
            world.flush();
        }
        serverbound::PlayPacket::SetHeldItem(p) => {
            world.trigger(PlayerChangeSlotEvent {
                entity,
                slot: p.slot,
            });
            world.flush();
        }
        serverbound::PlayPacket::CloseContainer(p) => {
            world.trigger(PlayerCloseContainerEvent {
                entity,
                window_id: p.window_id,
            });
            world.flush();
        }
        serverbound::PlayPacket::PlayerCommand(p) => match p.action_id {
            PlayerCommandAction::StartSneaking => {
                world.trigger(PlayerSneakEvent {
                    entity,
                    sneaking: true,
                });
                world.flush();
            }
            PlayerCommandAction::StopSneaking => {
                world.trigger(PlayerSneakEvent {
                    entity,
                    sneaking: false,
                });
                world.flush();
            }
            PlayerCommandAction::StartSprinting => {
                world.trigger(PlayerSprintEvent {
                    entity,
                    sprinting: true,
                });
                world.flush();
            }
            PlayerCommandAction::StopSprinting => {
                world.trigger(PlayerSprintEvent {
                    entity,
                    sprinting: false,
                });
                world.flush();
            }
            _ => {}
        },
        serverbound::PlayPacket::PlayerAction(p) => match p.status {
            PlayerActionStatus::StartedDigging => {
                world.trigger(PlayerStartDiggingEvent {
                    entity,
                    position: p.position,
                    face: p.face,
                    sequence: p.sequence,
                });
                world.flush();
            }
            PlayerActionStatus::CancelledDigging => {
                world.trigger(PlayerCancelDiggingEvent {
                    entity,
                    position: p.position,
                    face: p.face,
                    sequence: p.sequence,
                });
                world.flush();
            }
            PlayerActionStatus::FinishedDigging => {
                world.trigger(PlayerFinishDiggingEvent {
                    entity,
                    position: p.position,
                    face: p.face,
                    sequence: p.sequence,
                });
                world.flush();
            }
            PlayerActionStatus::DropItemStack => {
                world.trigger(PlayerDropItemEvent {
                    entity,
                    drop_stack: true,
                });
                world.flush();
            }
            PlayerActionStatus::DropItem => {
                world.trigger(PlayerDropItemEvent {
                    entity,
                    drop_stack: false,
                });
                world.flush();
            }
            PlayerActionStatus::SwapItemInHand => {
                world.trigger(PlayerSwapHandsEvent { entity });
                world.flush();
            }
            _ => {}
        },
        serverbound::PlayPacket::UseItemOn(p) => {
            world.trigger(PlayerUseItemOnBlockEvent {
                entity,
                hand: p.hand,
                position: p.location,
                face: p.face,
                cursor_x: p.cursor_x,
                cursor_y: p.cursor_y,
                cursor_z: p.cursor_z,
                inside_block: p.inside_block,
                sequence: p.sequence,
            });
            world.flush();
        }
        serverbound::PlayPacket::UseItem(p) => {
            world.trigger(PlayerUseItemEvent {
                entity,
                hand: p.hand,
                sequence: p.sequence,
            });
            world.flush();
        }
        serverbound::PlayPacket::Interact(p) => {
            handle_interact(world, entity, p);
        }
        _ => {}
    }
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

fn handle_command_suggestions(world: &mut World, client_id: u32, text: &str, transaction_id: i32) {
    // text is e.g. "/kick dan" — split into command + partial arg
    let without_slash = text.strip_prefix('/').unwrap_or(text);
    let parts: Vec<&str> = without_slash.splitn(2, ' ').collect();
    let command_name = parts[0];

    // Verify the command exists
    let exists = world
        .resource::<CommandRegistry>()
        .resolve(command_name)
        .is_some();
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

fn handle_interact(world: &mut World, entity: Entity, packet: &serverbound::Interact) {
    let mut data = packet._data.as_slice();

    let (attack, hand, target_pos) = match packet.interact_type {
        0 => {
            // Interact: hand + sneaking
            let hand = match Hand::decode(&mut data) {
                Ok(h) => Some(h),
                Err(_) => return,
            };
            (false, hand, None)
        }
        1 => {
            // Attack: sneaking only
            (true, None, None)
        }
        2 => {
            // Interact at: target_x/y/z + hand + sneaking
            let tx = match f32::decode(&mut data) {
                Ok(v) => v,
                Err(_) => return,
            };
            let ty = match f32::decode(&mut data) {
                Ok(v) => v,
                Err(_) => return,
            };
            let tz = match f32::decode(&mut data) {
                Ok(v) => v,
                Err(_) => return,
            };
            let hand = match Hand::decode(&mut data) {
                Ok(h) => Some(h),
                Err(_) => return,
            };
            (false, hand, Some((tx, ty, tz)))
        }
        _ => return,
    };

    let sneaking = match bool::decode(&mut data) {
        Ok(v) => v,
        Err(_) => false,
    };

    world.trigger(PlayerInteractEntityEvent {
        entity,
        target_id: packet.entity_id,
        attack,
        hand,
        target_pos,
        sneaking,
    });
    world.flush();
}
