pub mod chunk;
pub mod circle;
pub mod entities;
pub mod keep_alive;
pub mod physics;
pub mod player;
pub mod position;
pub mod settle;
pub mod wander;

use bevy_app::{App, Plugin, PostUpdate, Update};
use bevy_ecs::schedule::IntoScheduleConfigs;

use crate::commands::plugin::CommandSystems;

pub use keep_alive::KeepAliveTicker;

pub struct GameSystemsPlugin;

impl Plugin for GameSystemsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<KeepAliveTicker>()
            .add_observer(player::on_player_ready)
            .add_observer(entities::on_player_ready_spawn_entities)
            .add_observer(entities::on_entity_despawn)
            .add_observer(player::on_player_quit)
            .add_systems(
                Update,
                (
                    keep_alive::send_keep_alive.after(CommandSystems::DrainQueue),
                    wander::wander_system.after(keep_alive::send_keep_alive),
                    physics::apply_spawned_entity_physics.after(wander::wander_system),
                    circle::circle_system.after(keep_alive::send_keep_alive),
                    settle::settle_recent_spawns.after(physics::apply_spawned_entity_physics),
                ),
            )
            .add_systems(
                PostUpdate,
                (
                    entities::broadcast_entity_spawns,
                    entities::broadcast_entity_movement,
                    entities::broadcast_entity_motion,
                    entities::update_previous_entity_positions
                        .after(entities::broadcast_entity_movement),
                    position::broadcast_position,
                    position::update_previous_positions.after(position::broadcast_position),
                    chunk::stream_chunks,
                ),
            );
    }
}
