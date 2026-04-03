pub mod chunk;
pub mod keep_alive;
pub mod player;
pub mod position;

use bevy_app::{App, Plugin, PostUpdate, Update};
use bevy_ecs::schedule::IntoScheduleConfigs;

pub use keep_alive::KeepAliveTicker;

pub struct GameSystemsPlugin;

impl Plugin for GameSystemsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<KeepAliveTicker>()
            .add_systems(Update, keep_alive::send_keep_alive)
            .add_systems(
                PostUpdate,
                (
                    position::broadcast_position,
                    position::update_previous_positions.after(position::broadcast_position),
                    chunk::stream_chunks,
                ),
            );
    }
}
