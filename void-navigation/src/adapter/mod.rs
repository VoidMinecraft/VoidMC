//! The VoidMC binding of the navigation core. This is the only module that
//! knows about Bevy, chunks and entity components; porting the crate to a
//! redesigned engine means rewriting it and nothing under [`crate::pathing`].

mod behaviour;
mod blocks;
mod navigator;
mod systems;
mod world;

#[cfg(test)]
mod tests;

use bevy_app::{App, Plugin, Update};
use bevy_ecs::schedule::{IntoScheduleConfigs, SystemSet};
use voidmc::VoidSystems;
use voidmc::systems::physics::apply_spawned_entity_physics;
use voidmc::systems::wander::wander_system;

pub use behaviour::{Behaviour, BehaviourContext, Behaviours, Choice};
pub use blocks::{BlockModel, CellTable, classify};
pub use navigator::{DEFAULT_SPEED, Goal, NavigationEvent, NavigationOutcome, Navigator};
pub use systems::{NavigationSettings, NavigationStats, PathPlanner};
pub use world::{ChunkCells, DimensionCells, NavigationWorld};

/// Runs every navigation system, after the built-in wander AI and before the
/// entity physics step of the same tick.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NavigationSystems;

#[derive(Clone, Debug, Default)]
pub struct NavigationPlugin {
    model: BlockModel,
    settings: NavigationSettings,
}

impl NavigationPlugin {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn block_model(mut self, model: BlockModel) -> Self {
        self.model = model;
        self
    }

    pub fn expansions_per_tick(mut self, expansions: u32) -> Self {
        self.settings.expansions_per_tick = expansions;
        self
    }

    pub fn settings(mut self, settings: NavigationSettings) -> Self {
        self.settings = settings;
        self
    }
}

impl Plugin for NavigationPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(world::NavigationWorld::new(self.model))
            .insert_resource(self.settings.clone())
            .init_resource::<PathPlanner>()
            .init_resource::<NavigationStats>()
            .init_resource::<behaviour::PlayerSnapshot>()
            .add_observer(world::evict_unloaded_chunk)
            .add_observer(systems::release_velocity)
            .add_observer(behaviour::stagger_behaviours)
            .configure_sets(
                Update,
                NavigationSystems
                    .in_set(VoidSystems::EntitySimulation)
                    .after(wander_system)
                    .before(apply_spawned_entity_physics),
            )
            .add_systems(
                Update,
                (
                    world::invalidate_changed_chunks,
                    behaviour::select_behaviours,
                    systems::update_goals,
                    systems::schedule_searches,
                    systems::run_planner,
                    systems::follow_paths,
                    systems::emit_outcomes,
                )
                    .chain()
                    .in_set(NavigationSystems),
            );
    }
}
