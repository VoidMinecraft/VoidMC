use bevy_app::{App, Plugin};
use bevy_ecs::{observer::On, system::Commands, world::World};
use voidmc_protocol::serverbound::{
    ConfirmTeleportation, PlayerAbilities, SetPlayerPos, SetPlayerPosAndRot, SetPlayerRotation,
};

use crate::{
    components::{Position, Rotation, TeleportState},
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
