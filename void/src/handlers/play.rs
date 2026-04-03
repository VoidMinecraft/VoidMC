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
