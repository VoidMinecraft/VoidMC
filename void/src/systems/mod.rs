pub mod chunk;
pub mod entities;
pub mod keep_alive;
pub mod player;
pub mod position;

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
                keep_alive::send_keep_alive.after(CommandSystems::DrainQueue),
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
