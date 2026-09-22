use bevy_app::{App, Plugin};
use bevy_ecs::{observer::On, system::Commands, world::World};
use voidmc_protocol::serverbound::{
    ConfirmTeleportation, PlayerAbilities, SetPlayerPos, SetPlayerPosAndRot, SetPlayerRotation,
};

use crate::{
    components::{Position, Rotation, ServerControlledPosition, TeleportState},
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

fn handle_set_player_pos(
    event: On<PacketEvent<SetPlayerPos>>,
    world: &World,
    mut commands: Commands,
) {
    if world
        .get::<ServerControlledPosition>(event.entity)
        .is_some()
    {
        return;
    }
    let old_position = world.get::<Position>(event.entity).cloned();

    commands.entity(event.entity).insert(Position {
        x: event.packet.x,
        y: event.packet.y,
        z: event.packet.z,
    });

    if let Some(old) = old_position {
        commands.trigger(PlayerMoveEvent {
            entity: event.entity,
            old_x: old.x,
            old_y: old.y,
            old_z: old.z,
            new_x: event.packet.x,
            new_y: event.packet.y,
            new_z: event.packet.z,
        });
    }
}

fn handle_set_player_pos_and_rot(
    event: On<PacketEvent<SetPlayerPosAndRot>>,
    world: &World,
    mut commands: Commands,
) {
    if world
        .get::<ServerControlledPosition>(event.entity)
        .is_some()
    {
        commands.entity(event.entity).insert(Rotation {
            yaw: event.packet.yaw,
            pitch: event.packet.pitch,
        });
        commands.trigger(PlayerRotateEvent {
            entity: event.entity,
            yaw: event.packet.yaw,
            pitch: event.packet.pitch,
        });
        return;
    }
    let old_position = world.get::<Position>(event.entity).cloned();

    commands.entity(event.entity).insert((
        Position {
            x: event.packet.x,
            y: event.packet.y,
            z: event.packet.z,
        },
        Rotation {
            yaw: event.packet.yaw,
            pitch: event.packet.pitch,
        },
    ));

    if let Some(old) = old_position {
        commands.trigger(PlayerMoveEvent {
            entity: event.entity,
            old_x: old.x,
            old_y: old.y,
            old_z: old.z,
            new_x: event.packet.x,
            new_y: event.packet.y,
            new_z: event.packet.z,
        });
    }
    commands.trigger(PlayerRotateEvent {
        entity: event.entity,
        yaw: event.packet.yaw,
        pitch: event.packet.pitch,
    })
}

fn handle_set_player_rotation(
    event: On<PacketEvent<SetPlayerRotation>>,
    _world: &World,
    mut commands: Commands,
) {
    commands.entity(event.entity).insert(Rotation {
        yaw: event.packet.yaw,
        pitch: event.packet.pitch,
    });

    commands.trigger(PlayerRotateEvent {
        entity: event.entity,
        yaw: event.packet.yaw,
        pitch: event.packet.pitch,
    })
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
    use bevy_ecs::entity::Entity;

    use super::*;

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
    fn free_player_movement_is_unchanged() {
        let mut app = App::new();
        app.add_plugins(MovementPlugin);
        let entity = app.world_mut().spawn(Position::default()).id();
        move_to(&mut app, entity, 5.0, -5.0);
        let position = app.world().get::<Position>(entity).unwrap();
        assert_eq!((position.x, position.y, position.z), (5.0, 80.0, -5.0));
    }
}
