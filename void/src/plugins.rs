use bevy_app::Plugin;

pub mod chat;
pub mod configuration;
pub mod handshake;
pub mod interaction;
pub mod login;
pub mod movement;
pub mod play;
pub mod status;

pub struct DefaultPlugins;

impl Plugin for DefaultPlugins {
    fn build(&self, app: &mut bevy_app::App) {
        app.add_plugins((
            handshake::HandshakePlugin,
            status::StatusPlugin,
            login::LoginPlugin,
            configuration::ConfigurationPlugin,
            play::PlayPlugin,
            movement::MovementPlugin,
            chat::ChatPlugin,
            interaction::InteractionPlugin,
        ));
    }
}
