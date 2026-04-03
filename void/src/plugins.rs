use bevy_app::Plugin;

pub mod configuration;
pub mod handshake;
pub mod login;
pub mod movement;
pub mod status;

pub struct DefaultPlugins;

impl Plugin for DefaultPlugins {
    fn build(&self, app: &mut bevy_app::App) {
        app.add_plugins((
            handshake::HandshakePlugin,
            status::StatusPlugin,
            login::LoginPlugin,
            configuration::ConfigurationPlugin,
            movement::MovementPlugin,
        ));
    }
}
