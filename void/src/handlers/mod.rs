pub mod play;

use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::*;

use crate::events::{PacketQueue, PlayPacketEvent};
use crate::network::ingest_network_packets;
use crate::plugins::configuration::ConfigurationPlugin;
use crate::plugins::handshake::HandshakePlugin;
use crate::plugins::login::LoginPlugin;
use crate::plugins::status::StatusPlugin;
use crate::systems::player;

pub struct PlayPlugin;

impl Plugin for PlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreUpdate, handle_play_packets.after(ingest_network_packets));
    }
}

fn handle_play_packets(world: &mut World) {
    let packets = std::mem::take(&mut world.resource_mut::<PacketQueue<PlayPacketEvent>>().0);
    for event in packets {
        play::handle_play_packet(world, event.client_id, event.entity, event.packet);
    }
}

pub struct PlayerEventsPlugin;

impl Plugin for PlayerEventsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(player::on_player_ready)
            .add_observer(player::on_player_quit);
    }
}

/// Convenience plugin that registers all per-state handler plugins and player event observers.
pub struct DefaultHandlersPlugin;

impl Plugin for DefaultHandlersPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            HandshakePlugin,
            StatusPlugin,
            LoginPlugin,
            ConfigurationPlugin,
            PlayPlugin,
            PlayerEventsPlugin,
        ));
    }
}
