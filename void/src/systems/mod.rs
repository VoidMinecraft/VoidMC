pub mod chunk;
pub mod entities;
pub mod keep_alive;
pub mod physics;
pub mod player;
pub mod position;
pub mod settle;
pub mod wander;

use bevy_app::{App, Plugin, PostUpdate, Update};
use bevy_ecs::schedule::IntoScheduleConfigs;

use crate::schedule::VoidSystems;

pub use keep_alive::KeepAliveTicker;

pub struct GameSystemsPlugin;

impl Plugin for GameSystemsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<KeepAliveTicker>()
            .add_observer(player::on_player_ready)
            .add_observer(player::on_player_quit)
            .configure_sets(
                Update,
                (
                    VoidSystems::CommandDrain,
                    VoidSystems::KeepAlive,
                    VoidSystems::EntitySimulation,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    keep_alive::send_keep_alive.in_set(VoidSystems::KeepAlive),
                    (
                        settle::settle_recent_spawns,
                        wander::wander_system,
                        physics::apply_spawned_entity_physics,
                    )
                        .chain()
                        .in_set(VoidSystems::EntitySimulation),
                ),
            )
            .configure_sets(
                PostUpdate,
                (
                    VoidSystems::ChunkStreaming,
                    VoidSystems::EntityBroadcast,
                    VoidSystems::EntityVisibility,
                )
                    .chain(),
            )
            .add_systems(
                PostUpdate,
                (
                    (
                        entities::broadcast_entity_movement,
                        entities::broadcast_entity_motion,
                        entities::update_previous_entity_positions
                            .after(entities::broadcast_entity_movement),
                    )
                        .in_set(VoidSystems::EntityBroadcast),
                    (
                        position::broadcast_position,
                        position::update_previous_positions.after(position::broadcast_position),
                    )
                        .in_set(VoidSystems::PlayerBroadcast),
                    chunk::stream_chunks.in_set(VoidSystems::ChunkStreaming),
                ),
            );
    }
}
