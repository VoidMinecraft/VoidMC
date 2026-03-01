mod configuration;
mod handshake;
mod login;
mod play;
mod status;

use bevy_app::{App, Plugin, Update};

pub use configuration::RegistryDataStore;

pub struct HandlerPlugin;

impl Plugin for HandlerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                handshake::handle_handshake,
                status::handle_status,
                login::handle_login,
                configuration::handle_configuration,
                play::handle_play,
            ),
        );
    }
}
