use bevy_app::{App, Plugin};
use bevy_ecs::{entity::Entity, observer::On, system::Commands, world::World};
use voidmc_protocol::serverbound::{
    ConfirmTeleportation, PlayerAbilities, SetPlayerPos, SetPlayerPosAndRot, SetPlayerRotation,
};

use crate::{
    components::{Position, Rotation, TeleportState},
    entity::Mount,
    events::{PlayerMoveEvent, PlayerRotateEvent, PlayerToggleFlyEvent},
    network::PacketEvent,
};

pub struct MovementPlugin;

impl Plugin for MovementPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(handle_confirm_teleportation);
        app.add_observer(handle_set_player_pos);
        app.add_observer(handle_set_player_pos_and_rot);
        app.add_observer(handle_set_player_rotation);
        app.add_observer(handle_player_abilities);
    }
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

/// A riding client keeps sending its position (`y = -999`); like vanilla,
/// only the rotation of a passenger is trusted.
fn apply_position(world: &World, commands: &mut Commands, entity: Entity, new: Position) {
    if world.get::<Mount>(entity).is_some() {
        return;
    }
    let old_position = world.get::<Position>(entity).copied();
    commands.entity(entity).insert(new);
    if let Some(old) = old_position {
        commands.trigger(PlayerMoveEvent {
            entity,
            old_x: old.x,
            old_y: old.y,
            old_z: old.z,
            new_x: new.x,
            new_y: new.y,
            new_z: new.z,
        });
    }
}

fn apply_rotation(world: &World, commands: &mut Commands, entity: Entity, new: Rotation) {
    if world.get::<Rotation>(entity) != Some(&new) {
        commands.entity(entity).insert(new);
    }
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
        world,
        &mut commands,
        event.entity,
        Rotation {
            yaw: event.packet.yaw,
            pitch: event.packet.pitch,
        },
    );
}

fn handle_set_player_rotation(
    event: On<PacketEvent<SetPlayerRotation>>,
    world: &World,
    mut commands: Commands,
) {
    apply_rotation(
        world,
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
            .add_plugins(MovementPlugin)
            .add_systems(bevy_app::PostUpdate, count_rotation_writes)
            .add_observer(|_: On<PlayerMoveEvent>, mut moves: ResMut<Moves>| moves.0 += 1)
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
}
