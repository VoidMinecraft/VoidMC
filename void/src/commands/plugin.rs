use bevy_app::{App, Plugin, Update};
use bevy_ecs::schedule::{IntoScheduleConfigs, SystemSet};

use super::{CommandEnqueueSequence, CommandQueue, CommandRegistry, drain_command_queue};

#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
pub enum CommandSystems {
    DrainQueue,
}

/// Plugin that initializes the command system.
/// The CommandRegistry starts empty; use `register_default_commands()`
/// or `VoidServer::add_command()` to populate it.
pub struct CommandPlugin;

impl Plugin for CommandPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CommandRegistry>()
            .init_resource::<CommandQueue>()
            .init_resource::<CommandEnqueueSequence>()
            .add_systems(
                Update,
                drain_command_queue.in_set(CommandSystems::DrainQueue),
            );
    }
}
