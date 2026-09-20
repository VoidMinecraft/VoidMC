use bevy_app::{App, Plugin, Update};
use bevy_ecs::schedule::{IntoScheduleConfigs, SystemSet};

use super::{CommandEnqueueSequence, CommandQueue, CommandRegistry, drain_command_queue};
use crate::schedule::VoidSystems;

#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
pub enum CommandSystems {
    DrainQueue,
}

/// Ordering handle for the command drain. Contained in
/// [`VoidSystems::CommandDrain`]; kept for backwards compatibility.
///
/// Plugin that initializes the command system.
/// The CommandRegistry starts empty; use `register_default_commands()`
/// or `VoidServer::add_command()` to populate it.
pub struct CommandPlugin;

impl Plugin for CommandPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CommandRegistry>()
            .init_resource::<CommandQueue>()
            .init_resource::<CommandEnqueueSequence>()
            .configure_sets(
                Update,
                CommandSystems::DrainQueue.in_set(VoidSystems::CommandDrain),
            )
            .add_systems(
                Update,
                drain_command_queue.in_set(CommandSystems::DrainQueue),
            );
    }
}
