use bevy_app::{App, Plugin};
use bevy_ecs::{component::Component, observer::On, system::Commands, world::EntityWorldMut};
use voidmc_protocol::{
    serverbound::{
        Attack, CloseContainer, Interact, PlayerAction, PlayerCommand, PlayerInput, SetHeldItem,
        SwingArm, UseItem, UseItemOn,
    },
    types::{PlayerActionStatus, PlayerCommandAction},
};

use crate::{
    events::{
        PlayerCancelDiggingEvent, PlayerChangeSlotEvent, PlayerCloseContainerEvent,
        PlayerDropItemEvent, PlayerFinishDiggingEvent, PlayerInputEvent, PlayerInteractEntityEvent,
        PlayerSneakEvent, PlayerSprintEvent, PlayerStartDiggingEvent, PlayerSwapHandsEvent,
        PlayerSwingArmEvent, PlayerUseItemEvent, PlayerUseItemOnBlockEvent,
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
        app.add_observer(handle_player_input);
        app.add_observer(handle_player_action);
        app.add_observer(handle_use_item);
        app.add_observer(handle_use_item_on);
        app.add_observer(handle_interact);
        app.add_observer(handle_attack);
    }
}

/// The sneak key state from the client's last Player Input, so a sneak
/// event fires only when it changes.
#[derive(Component, Default)]
struct SneakKey(bool);

fn handle_player_input(event: On<PacketEvent<PlayerInput>>, mut commands: Commands) {
    let input = event.packet;
    let entity = event.entity;
    commands.trigger(PlayerInputEvent {
        entity,
        forward: input.forward(),
        backward: input.backward(),
        left: input.left(),
        right: input.right(),
        jump: input.jump(),
        sneak: input.sneak(),
        sprint: input.sprint(),
    });
    // Since 1.21.2 the sneak key is reported here, not by Player Command.
    let sneaking = input.sneak();
    commands
        .entity(entity)
        .queue(move |mut player: EntityWorldMut| {
            let changed = match player.get_mut::<SneakKey>() {
                Some(mut held) => std::mem::replace(&mut held.0, sneaking) != sneaking,
                None => {
                    player.insert(SneakKey(sneaking));
                    sneaking
                }
            };
            if changed {
                player.world_scope(|world| world.trigger(PlayerSneakEvent { entity, sneaking }));
            }
        });
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
        container_id: event.packet.container_id,
    })
}

fn handle_player_command(event: On<PacketEvent<PlayerCommand>>, mut commands: Commands) {
    let sprinting = match event.packet.action_id {
        PlayerCommandAction::StartSprinting => true,
        PlayerCommandAction::StopSprinting => false,
        _ => return,
    };
    commands.trigger(PlayerSprintEvent {
        entity: event.entity,
        sprinting,
    });
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
    let packet = &event.packet;
    let location = packet.location;
    commands.trigger(PlayerInteractEntityEvent {
        entity: event.entity,
        target_id: packet.entity_id,
        attack: false,
        hand: Some(packet.hand),
        target_pos: Some((location.x as f32, location.y as f32, location.z as f32)),
        sneaking: packet.using_secondary_action,
    });
}

/// Since 26.1 an attack is its own packet, which carries only the target.
fn handle_attack(event: On<PacketEvent<Attack>>, mut commands: Commands) {
    commands.trigger(PlayerInteractEntityEvent {
        entity: event.entity,
        target_id: event.packet.entity_id,
        attack: true,
        hand: None,
        target_pos: None,
        sneaking: false,
    });
}

#[cfg(test)]
mod tests {
    use bevy_ecs::prelude::*;

    use super::*;

    #[derive(Resource, Default)]
    struct Seen(Vec<String>);

    fn app() -> App {
        let mut app = App::new();
        app.init_resource::<Seen>()
            .add_plugins(InteractionPlugin)
            .add_observer(|event: On<PlayerSneakEvent>, mut seen: ResMut<Seen>| {
                seen.0.push(format!("sneak {}", event.sneaking));
            })
            .add_observer(|event: On<PlayerSprintEvent>, mut seen: ResMut<Seen>| {
                seen.0.push(format!("sprint {}", event.sprinting));
            })
            .add_observer(|event: On<PlayerInputEvent>, mut seen: ResMut<Seen>| {
                seen.0.push(format!("input sneak={}", event.sneak));
            });
        app
    }

    fn take(app: &mut App) -> Vec<String> {
        std::mem::take(&mut app.world_mut().resource_mut::<Seen>().0)
    }

    fn input(app: &mut App, entity: Entity, flags: u8) -> Vec<String> {
        app.world_mut().trigger(PacketEvent {
            client_id: 1,
            entity,
            packet: PlayerInput { flags },
        });
        app.world_mut().flush();
        take(app)
    }

    fn command(app: &mut App, entity: Entity, action_id: PlayerCommandAction) -> Vec<String> {
        app.world_mut().trigger(PacketEvent {
            client_id: 1,
            entity,
            packet: PlayerCommand {
                entity_id: 1,
                action_id,
                jump_boost: 0,
            },
        });
        app.world_mut().flush();
        take(app)
    }

    #[test]
    fn the_sneak_key_in_player_input_fires_a_sneak_event_when_it_changes() {
        let mut app = app();
        let player = app.world_mut().spawn_empty().id();

        assert_eq!(
            input(&mut app, player, PlayerInput::FORWARD),
            ["input sneak=false"]
        );
        assert_eq!(
            input(&mut app, player, PlayerInput::SNEAK),
            ["input sneak=true", "sneak true"]
        );
        assert_eq!(
            input(&mut app, player, PlayerInput::SNEAK | PlayerInput::FORWARD),
            ["input sneak=true"]
        );
        assert_eq!(
            input(&mut app, player, PlayerInput::FORWARD),
            ["input sneak=false", "sneak false"]
        );
        assert_eq!(input(&mut app, player, 0), ["input sneak=false"]);
    }

    #[test]
    fn a_player_already_sneaking_in_its_first_input_starts_sneaking() {
        let mut app = app();
        let player = app.world_mut().spawn_empty().id();
        assert_eq!(
            input(&mut app, player, PlayerInput::SNEAK),
            ["input sneak=true", "sneak true"]
        );
    }

    #[test]
    fn player_commands_report_sprinting_and_never_sneaking() {
        let mut app = app();
        let player = app.world_mut().spawn_empty().id();

        assert_eq!(
            command(&mut app, player, PlayerCommandAction::StartSprinting),
            ["sprint true"]
        );
        assert_eq!(
            command(&mut app, player, PlayerCommandAction::StopSprinting),
            ["sprint false"]
        );
        for other in [
            PlayerCommandAction::LeaveBed,
            PlayerCommandAction::StartJumpWithHorse,
            PlayerCommandAction::StopJumpWithHorse,
            PlayerCommandAction::OpenVehicleInventory,
            PlayerCommandAction::StartFlyingWithElytra,
        ] {
            assert!(command(&mut app, player, other).is_empty(), "{other:?}");
        }
    }
}
