use bevy_app::{App, Plugin};
use bevy_ecs::{
    change_detection::DetectChangesMut,
    component::{Component, Mutable},
    entity::Entity,
    observer::On,
    system::Commands,
    world::{EntityWorldMut, World},
};
use voidmc_protocol::serverbound::{
    ConfirmTeleportation, PlayerAbilities, PlayerInput, SetPlayerPos, SetPlayerPosAndRot,
    SetPlayerRotation,
};

use crate::{
    components::{Grounded, Position, Rotation, ServerControlledPosition, Sneaking, TeleportState},
    entity::Mount,
    events::{PlayerMoveEvent, PlayerRotateEvent, PlayerToggleFlyEvent},
    network::PacketEvent,
};

/// Bit of the movement packets' flags byte set while the client stands on the ground.
const ON_GROUND: u8 = 0x01;

pub struct MovementPlugin;

impl Plugin for MovementPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(handle_confirm_teleportation);
        app.add_observer(handle_set_player_pos);
        app.add_observer(handle_set_player_pos_and_rot);
        app.add_observer(handle_set_player_rotation);
        app.add_observer(handle_player_abilities);
        app.add_observer(handle_player_input);
    }
}

/// Inserts `value`, or overwrites it only when it differs, so change
/// detection fires on real changes only.
fn set_if_changed<C: Component<Mutability = Mutable> + PartialEq>(
    commands: &mut Commands,
    entity: Entity,
    value: C,
) {
    commands
        .entity(entity)
        .queue(move |mut e: EntityWorldMut| match e.get_mut::<C>() {
            Some(mut current) => {
                current.set_if_neq(value);
            }
            None => {
                e.insert(value);
            }
        });
}

fn apply_on_ground(commands: &mut Commands, entity: Entity, flags: u8) {
    set_if_changed(commands, entity, Grounded(flags & ON_GROUND != 0));
}

fn handle_player_input(event: On<PacketEvent<PlayerInput>>, mut commands: Commands) {
    set_if_changed(&mut commands, event.entity, Sneaking(event.packet.sneak()));
}

fn handle_confirm_teleportation(
    event: On<PacketEvent<ConfirmTeleportation>>,
    world: &World,
    mut commands: Commands,
) {
    if let Some(teleport_state) = world.get::<TeleportState>(event.entity) {
        if teleport_state.pending_id == Some(event.packet.teleport_id) {
            commands.entity(event.entity).insert(TeleportState {
                pending_id: None,
                ..*teleport_state
            });
        } else {
            tracing::warn!(
                "Client {} confirmed teleportation with unexpected ID {}, expected {:?}",
                event.client_id,
                event.packet.teleport_id,
                teleport_state.pending_id
            );
        }
    }
}

/// Like vanilla, positions are ignored while a position sync is unconfirmed:
/// they were sent before the client processed the teleport and would undo it.
fn apply_position(world: &World, commands: &mut Commands, entity: Entity, new: Position) {
    if world.get::<Mount>(entity).is_some()
        || world.get::<ServerControlledPosition>(entity).is_some()
        || world
            .get::<TeleportState>(entity)
            .is_some_and(|state| state.pending_id.is_some())
    {
        return;
    }
    commands.queue(move |world: &mut World| {
        let Ok(mut entity_mut) = world.get_entity_mut(entity) else {
            return;
        };
        let old = entity_mut.get::<Position>().copied();
        entity_mut.insert(new);
        if let Some(old) = old {
            world.trigger(PlayerMoveEvent {
                entity,
                old_x: old.x,
                old_y: old.y,
                old_z: old.z,
                new_x: new.x,
                new_y: new.y,
                new_z: new.z,
            });
        }
    });
}

fn apply_rotation(commands: &mut Commands, entity: Entity, new: Rotation) {
    set_if_changed(commands, entity, new);
    commands.trigger(PlayerRotateEvent {
        entity,
        yaw: new.yaw,
        pitch: new.pitch,
    });
}

fn handle_set_player_pos(
    event: On<PacketEvent<SetPlayerPos>>,
    world: &World,
    mut commands: Commands,
) {
    apply_on_ground(&mut commands, event.entity, event.packet.flags);
    apply_position(
        world,
        &mut commands,
        event.entity,
        Position {
            x: event.packet.x,
            y: event.packet.y,
            z: event.packet.z,
        },
    );
}

fn handle_set_player_pos_and_rot(
    event: On<PacketEvent<SetPlayerPosAndRot>>,
    world: &World,
    mut commands: Commands,
) {
    apply_on_ground(&mut commands, event.entity, event.packet.flags);
    apply_position(
        world,
        &mut commands,
        event.entity,
        Position {
            x: event.packet.x,
            y: event.packet.y,
            z: event.packet.z,
        },
    );
    apply_rotation(
        &mut commands,
        event.entity,
        Rotation {
            yaw: event.packet.yaw,
            pitch: event.packet.pitch,
        },
    );
}

fn handle_set_player_rotation(event: On<PacketEvent<SetPlayerRotation>>, mut commands: Commands) {
    apply_on_ground(&mut commands, event.entity, event.packet.flags);
    apply_rotation(
        &mut commands,
        event.entity,
        Rotation {
            yaw: event.packet.yaw,
            pitch: event.packet.pitch,
        },
    );
}

fn handle_player_abilities(event: On<PacketEvent<PlayerAbilities>>, mut commands: Commands) {
    let flying = (event.packet.flags & 0x02) != 0;
    commands.trigger(PlayerToggleFlyEvent {
        entity: event.entity,
        flying,
    });
}

#[cfg(test)]
mod tests {
    use bevy_app::App;
    use bevy_ecs::prelude::*;

    use super::*;

    #[derive(Resource, Default)]
    struct Moves(u32);

    #[derive(Resource, Default)]
    struct LastMove(Option<(f64, f64)>);

    #[derive(Resource, Default)]
    struct Rotates(u32);

    #[derive(Resource, Default)]
    struct RotationWrites(u32);

    fn count_rotation_writes(
        changed: Query<(), Changed<Rotation>>,
        mut writes: ResMut<RotationWrites>,
    ) {
        writes.0 += changed.iter().count() as u32;
    }

    fn test_app() -> App {
        let mut app = App::new();
        app.init_resource::<Moves>()
            .init_resource::<Rotates>()
            .init_resource::<RotationWrites>()
            .init_resource::<LastMove>()
            .add_plugins(MovementPlugin)
            .add_systems(bevy_app::PostUpdate, count_rotation_writes)
            .add_observer(
                |event: On<PlayerMoveEvent>,
                 mut moves: ResMut<Moves>,
                 mut last: ResMut<LastMove>| {
                    moves.0 += 1;
                    last.0 = Some((event.old_x, event.new_x));
                },
            )
            .add_observer(|_: On<PlayerRotateEvent>, mut rotates: ResMut<Rotates>| rotates.0 += 1);
        app
    }

    fn pos_and_rot(app: &mut App, entity: Entity, y: f64, yaw: f32) {
        app.world_mut().trigger(PacketEvent {
            client_id: 1,
            entity,
            packet: SetPlayerPosAndRot {
                x: 0.25,
                y,
                z: -0.5,
                yaw,
                pitch: 10.0,
                flags: 0,
            },
        });
        app.update();
    }

    #[test]
    fn a_passenger_keeps_its_position_but_turns() {
        let mut app = test_app();
        let vehicle = app.world_mut().spawn_empty().id();
        let rider = app
            .world_mut()
            .spawn((
                Position {
                    x: 10.0,
                    y: 64.0,
                    z: 10.0,
                },
                Rotation::default(),
                Mount(vehicle),
            ))
            .id();

        pos_and_rot(&mut app, rider, -999.0, 90.0);

        let pos = app.world().get::<Position>(rider).unwrap();
        assert_eq!((pos.x, pos.y, pos.z), (10.0, 64.0, 10.0));
        let rot = app.world().get::<Rotation>(rider).unwrap();
        assert_eq!((rot.yaw, rot.pitch), (90.0, 10.0));
        assert_eq!(app.world().resource::<Moves>().0, 0);
        assert_eq!(app.world().resource::<Rotates>().0, 1);

        app.world_mut().entity_mut(rider).remove::<Mount>();
        pos_and_rot(&mut app, rider, 65.0, 90.0);
        let pos = app.world().get::<Position>(rider).unwrap();
        assert_eq!((pos.x, pos.y, pos.z), (0.25, 65.0, -0.5));
        assert_eq!(app.world().resource::<Moves>().0, 1);
    }

    #[test]
    fn an_unchanged_rotation_is_not_rewritten() {
        let mut app = test_app();
        let walker = app
            .world_mut()
            .spawn((Position::default(), Rotation::default()))
            .id();
        app.update();
        assert_eq!(app.world().resource::<RotationWrites>().0, 1);

        pos_and_rot(&mut app, walker, 64.0, 90.0);
        assert_eq!(app.world().resource::<RotationWrites>().0, 2);

        pos_and_rot(&mut app, walker, 64.0, 90.0);
        assert_eq!(app.world().resource::<RotationWrites>().0, 2);
        assert_eq!(app.world().resource::<Rotates>().0, 2);
    }

    #[test]
    fn two_rotation_packets_in_one_tick_keep_the_last() {
        let mut app = test_app();
        let walker = app
            .world_mut()
            .spawn((Position::default(), Rotation::default()))
            .id();
        app.update();

        for yaw in [90.0, 0.0] {
            app.world_mut().trigger(PacketEvent {
                client_id: 1,
                entity: walker,
                packet: SetPlayerRotation {
                    yaw,
                    pitch: 0.0,
                    flags: 0,
                },
            });
        }
        app.update();

        let rot = app.world().get::<Rotation>(walker).unwrap();
        assert_eq!((rot.yaw, rot.pitch), (0.0, 0.0));
        assert_eq!(app.world().resource::<Rotates>().0, 2);
    }

    #[test]
    fn two_move_packets_in_one_tick_report_the_real_previous_position() {
        let mut app = test_app();
        let walker = app
            .world_mut()
            .spawn((Position::default(), Rotation::default()))
            .id();
        app.update();

        for x in [1.0, 2.0] {
            app.world_mut().trigger(PacketEvent {
                client_id: 1,
                entity: walker,
                packet: SetPlayerPos {
                    x,
                    y: 64.0,
                    z: 0.0,
                    flags: 0,
                },
            });
        }
        app.update();

        assert_eq!(app.world().resource::<Moves>().0, 2);
        assert_eq!(app.world().resource::<LastMove>().0, Some((1.0, 2.0)));
        assert_eq!(app.world().get::<Position>(walker).unwrap().x, 2.0);
    }

    fn move_to(app: &mut App, entity: Entity, x: f64, z: f64) {
        app.world_mut().trigger(PacketEvent {
            client_id: 1,
            entity,
            packet: SetPlayerPos {
                x,
                y: 80.0,
                z,
                flags: 0,
            },
        });
        app.world_mut().flush();
    }

    #[test]
    fn server_controlled_position_ignores_client_movement_but_keeps_look() {
        let mut app = App::new();
        app.add_plugins(MovementPlugin);
        let entity = app
            .world_mut()
            .spawn((
                Position {
                    x: 1.0,
                    y: 80.0,
                    z: 2.0,
                },
                ServerControlledPosition,
            ))
            .id();

        move_to(&mut app, entity, 999.0, 999.0);
        assert_eq!(app.world().get::<Position>(entity).unwrap().x, 1.0);

        app.world_mut().trigger(PacketEvent {
            client_id: 1,
            entity,
            packet: SetPlayerPosAndRot {
                x: 999.0,
                y: 0.0,
                z: 999.0,
                yaw: 45.0,
                pitch: 15.0,
                flags: 0,
            },
        });
        app.world_mut().flush();
        assert_eq!(app.world().get::<Position>(entity).unwrap().x, 1.0);
        assert_eq!(app.world().get::<Rotation>(entity).unwrap().yaw, 45.0);

        app.world_mut()
            .entity_mut(entity)
            .remove::<ServerControlledPosition>();
        move_to(&mut app, entity, 3.0, 2.0);
        assert_eq!(app.world().get::<Position>(entity).unwrap().x, 3.0);
    }

    #[test]
    fn positions_sent_before_a_teleport_is_confirmed_are_ignored() {
        let mut app = test_app();
        let entity = app
            .world_mut()
            .spawn((
                Position {
                    x: 100.0,
                    y: 80.0,
                    z: 100.0,
                },
                TeleportState {
                    next_id: 6,
                    pending_id: Some(5),
                },
            ))
            .id();

        move_to(&mut app, entity, 1.0, 1.0);
        assert_eq!(app.world().get::<Position>(entity).unwrap().x, 100.0);
        app.world_mut().trigger(PacketEvent {
            client_id: 1,
            entity,
            packet: ConfirmTeleportation { teleport_id: 4 },
        });
        app.world_mut().flush();
        move_to(&mut app, entity, 1.0, 1.0);
        assert_eq!(app.world().get::<Position>(entity).unwrap().x, 100.0);
        assert_eq!(app.world().resource::<Moves>().0, 0);

        app.world_mut().trigger(PacketEvent {
            client_id: 1,
            entity,
            packet: ConfirmTeleportation { teleport_id: 5 },
        });
        app.world_mut().flush();
        move_to(&mut app, entity, 101.0, 100.0);
        assert_eq!(app.world().get::<Position>(entity).unwrap().x, 101.0);
        assert_eq!(app.world().resource::<Moves>().0, 1);
    }

    #[test]
    fn rotations_and_ground_flags_sent_before_a_teleport_is_confirmed_are_ignored() {
        let mut app = test_app();
        let entity = app
            .world_mut()
            .spawn((
                Position::default(),
                Rotation::default(),
                Grounded(false),
                TeleportState {
                    next_id: 6,
                    pending_id: Some(5),
                },
            ))
            .id();
        let state = |app: &App| {
            let rot = app.world().get::<Rotation>(entity).unwrap();
            (
                (rot.yaw, rot.pitch),
                app.world().get::<Grounded>(entity).unwrap().0,
                app.world().resource::<Rotates>().0,
            )
        };

        pos_and_rot(&mut app, entity, 64.0, 90.0);
        app.world_mut().trigger(PacketEvent {
            client_id: 1,
            entity,
            packet: SetPlayerRotation {
                yaw: 45.0,
                pitch: 20.0,
                flags: ON_GROUND,
            },
        });
        app.update();
        assert_eq!(state(&app), ((0.0, 0.0), false, 0));

        app.world_mut().trigger(PacketEvent {
            client_id: 1,
            entity,
            packet: ConfirmTeleportation { teleport_id: 5 },
        });
        app.world_mut().flush();
        app.world_mut().trigger(PacketEvent {
            client_id: 1,
            entity,
            packet: SetPlayerRotation {
                yaw: 45.0,
                pitch: 20.0,
                flags: ON_GROUND,
            },
        });
        app.update();
        assert_eq!(state(&app), ((45.0, 20.0), true, 1));
    }

    #[test]
    fn free_player_movement_is_unchanged() {
        let mut app = App::new();
        app.add_plugins(MovementPlugin);
        let entity = app.world_mut().spawn(Position::default()).id();
        move_to(&mut app, entity, 5.0, -5.0);
        let position = app.world().get::<Position>(entity).unwrap();
        assert_eq!((position.x, position.y, position.z), (5.0, 80.0, -5.0));
    }

    #[test]
    fn movement_flags_and_input_record_ground_and_sneak() {
        let mut app = App::new();
        app.add_plugins(MovementPlugin);
        let entity = app.world_mut().spawn(Position::default()).id();
        let grounded = |app: &App| app.world().get::<Grounded>(entity).unwrap().0;
        let sneaking = |app: &App| app.world().get::<Sneaking>(entity).unwrap().0;

        move_to(&mut app, entity, 0.0, 0.0);
        assert!(!grounded(&app), "flags 0 is airborne");
        app.world_mut().trigger(PacketEvent {
            client_id: 1,
            entity,
            packet: SetPlayerRotation {
                yaw: 0.0,
                pitch: 0.0,
                flags: ON_GROUND,
            },
        });
        app.world_mut().flush();
        assert!(grounded(&app));

        for (flags, sneak) in [
            (PlayerInput::SNEAK | PlayerInput::FORWARD, true),
            (PlayerInput::FORWARD, false),
        ] {
            app.world_mut().trigger(PacketEvent {
                client_id: 1,
                entity,
                packet: PlayerInput { flags },
            });
            app.world_mut().flush();
            assert_eq!(sneaking(&app), sneak);
        }
    }
}
