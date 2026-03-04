use bevy_app::{App, Plugin};

use super::CommandRegistry;

/// Plugin that initializes the command system.
/// The CommandRegistry resource starts empty — use `register_default_commands()`
/// or `VoidServer::add_command()` to populate it.
///
/// Command dispatch is handled directly in the play packet handler
/// (which has exclusive `&mut World` access needed by command handlers).
pub struct CommandPlugin;

impl Plugin for CommandPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CommandRegistry>();
    }
}
