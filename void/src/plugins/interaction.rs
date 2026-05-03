use bevy_app::{App, Plugin};
use bevy_ecs::{observer::On, system::Commands};
use voidmc_codec::Decode;
use voidmc_protocol::{
    serverbound::{
        CloseContainer, Interact, PlayerAction, PlayerCommand, SetHeldItem, SwingArm, UseItem,
        UseItemOn,
    },
    types::{Hand, PlayerActionStatus, PlayerCommandAction},
};

use crate::{
    events::{
        PlayerCancelDiggingEvent, PlayerChangeSlotEvent, PlayerCloseContainerEvent,
        PlayerDropItemEvent, PlayerFinishDiggingEvent, PlayerInteractEntityEvent, PlayerSneakEvent,
        PlayerSprintEvent, PlayerStartDiggingEvent, PlayerSwapHandsEvent, PlayerSwingArmEvent,
        PlayerUseItemEvent, PlayerUseItemOnBlockEvent,
    },
    network::PacketEvent,
};

pub struct InteractionPlugin;

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(handle_swing_arm);
        app.add_observer(handle_set_held_item);
        app.add_observer(handle_close_container);
        app.add_observer(handle_player_command);
        app.add_observer(handle_player_action);
        app.add_observer(handle_use_item);
        app.add_observer(handle_use_item_on);
        app.add_observer(handle_interact);
    }
}

fn handle_swing_arm(event: On<PacketEvent<SwingArm>>, mut commands: Commands) {
    commands.trigger(PlayerSwingArmEvent {
        entity: event.entity,
        hand: event.packet.hand,
    })
}

fn handle_set_held_item(event: On<PacketEvent<SetHeldItem>>, mut commands: Commands) {
    commands.trigger(PlayerChangeSlotEvent {
        entity: event.entity,
        slot: event.packet.slot,
    })
}

fn handle_close_container(event: On<PacketEvent<CloseContainer>>, mut commands: Commands) {
    commands.trigger(PlayerCloseContainerEvent {
        entity: event.entity,
        window_id: event.packet.window_id,
    })
}

fn handle_player_command(event: On<PacketEvent<PlayerCommand>>, mut commands: Commands) {
    match event.packet.action_id {
        PlayerCommandAction::StartSneaking => {
            commands.trigger(PlayerSneakEvent {
                entity: event.entity,
                sneaking: true,
            });
        }
        PlayerCommandAction::StopSneaking => {
            commands.trigger(PlayerSneakEvent {
                entity: event.entity,
                sneaking: false,
            });
        }
        PlayerCommandAction::StartSprinting => {
            commands.trigger(PlayerSprintEvent {
                entity: event.entity,
                sprinting: true,
            });
        }
        PlayerCommandAction::StopSprinting => {
            commands.trigger(PlayerSprintEvent {
                entity: event.entity,
                sprinting: false,
            });
        }
        _ => {}
    }
}

fn handle_player_action(event: On<PacketEvent<PlayerAction>>, mut commands: Commands) {
    match event.packet.status {
        PlayerActionStatus::StartedDigging => {
            commands.trigger(PlayerStartDiggingEvent {
                entity: event.entity,
                position: event.packet.position,
                face: event.packet.face,
                sequence: event.packet.sequence,
            });
        }
        PlayerActionStatus::CancelledDigging => {
            commands.trigger(PlayerCancelDiggingEvent {
                entity: event.entity,
                position: event.packet.position,
                face: event.packet.face,
                sequence: event.packet.sequence,
            });
        }
        PlayerActionStatus::FinishedDigging => {
            commands.trigger(PlayerFinishDiggingEvent {
                entity: event.entity,
                position: event.packet.position,
                face: event.packet.face,
                sequence: event.packet.sequence,
            });
        }
        PlayerActionStatus::DropItemStack => {
            commands.trigger(PlayerDropItemEvent {
                entity: event.entity,
                drop_stack: true,
            });
        }
        PlayerActionStatus::DropItem => {
            commands.trigger(PlayerDropItemEvent {
                entity: event.entity,
                drop_stack: false,
            });
        }
        PlayerActionStatus::SwapItemInHand => {
            commands.trigger(PlayerSwapHandsEvent {
                entity: event.entity,
            });
        }
        _ => {}
    }
}

fn handle_use_item(event: On<PacketEvent<UseItem>>, mut commands: Commands) {
    commands.trigger(PlayerUseItemEvent {
        entity: event.entity,
        hand: event.packet.hand,
        sequence: event.packet.sequence,
    })
}

fn handle_use_item_on(event: On<PacketEvent<UseItemOn>>, mut commands: Commands) {
    commands.trigger(PlayerUseItemOnBlockEvent {
        entity: event.entity,
        hand: event.packet.hand,
        position: event.packet.location,
        face: event.packet.face,
        cursor_x: event.packet.cursor_x,
        cursor_y: event.packet.cursor_y,
        cursor_z: event.packet.cursor_z,
        inside_block: event.packet.inside_block,
        sequence: event.packet.sequence,
    })
}

fn handle_interact(event: On<PacketEvent<Interact>>, mut commands: Commands) {
    let mut data = event.packet._data.as_slice();

    let (attack, hand, target_pos) = match event.packet.interact_type {
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

    commands.trigger(PlayerInteractEntityEvent {
        entity: event.entity,
        target_id: event.packet.entity_id,
        attack,
        hand,
        target_pos,
        sneaking,
    });
}
