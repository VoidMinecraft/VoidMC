pub mod configuration;
pub mod handshake;
pub mod login;
pub mod play;
pub mod status;

use bevy_app::{App, Plugin};

use crate::systems::player;

pub struct HandlerPlugin;

impl Plugin for HandlerPlugin {
    fn build(&self, app: &mut App) {
        // Handlers are called directly from ingest_network_packets — no systems needed.
        // Register observers for semantic events.
        app.add_observer(player::on_player_ready)
            .add_observer(player::on_player_quit);
    }
}
