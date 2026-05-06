pub mod chunk;
pub mod circle;
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
            .add_observer(player::on_player_quit)
            .add_systems(
                Update,
                (
                    keep_alive::send_keep_alive.after(CommandSystems::DrainQueue),
                    wander::wander_system.after(keep_alive::send_keep_alive),
                    physics::apply_spawned_entity_physics.after(wander::wander_system),
                    circle::circle_system.after(keep_alive::send_keep_alive),
                    settle::settle_recent_spawns.after(physics::apply_spawned_entity_physics),
                    position::decrement_movement_cooldowns.after(physics::apply_spawned_entity_physics),
                ),
            )
            .add_systems(
                PostUpdate,
                (
                    position::force_updates_for_recently_spawned,
                    position::broadcast_player_position,
                    position::broadcast_spawned_position.after(position::force_updates_for_recently_spawned),
                    position::update_previous_player_positions.after(position::broadcast_player_position),
                    position::update_previous_spawned_positions.after(position::broadcast_spawned_position),
                    chunk::stream_chunks,
                ),
            );
    }
}
