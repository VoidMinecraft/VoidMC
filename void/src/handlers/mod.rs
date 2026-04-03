pub mod configuration;
pub mod handshake;
pub mod login;
pub mod play;
pub mod status;

use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::*;

use crate::events::{
    ConfigurationPacketEvent, HandshakePacketEvent, LoginPacketEvent, PacketQueue, PlayPacketEvent,
    StatusPacketEvent,
};
use crate::network::ingest_network_packets;
use crate::systems::player;

pub struct HandshakePlugin;

impl Plugin for HandshakePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PreUpdate,
            handle_handshake_packets.after(ingest_network_packets),
        );
    }
}

fn handle_handshake_packets(world: &mut World) {
    let packets = std::mem::take(&mut world.resource_mut::<PacketQueue<HandshakePacketEvent>>().0);
    for event in packets {
        handshake::handle_handshake_packet(world, event.client_id, event.entity, event.packet);
    }
}

pub struct StatusPlugin;

impl Plugin for StatusPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PreUpdate,
            handle_status_packets.after(ingest_network_packets),
        );
    }
}

fn handle_status_packets(world: &mut World) {
    let packets = std::mem::take(&mut world.resource_mut::<PacketQueue<StatusPacketEvent>>().0);
    for event in packets {
        status::handle_status_packet(world, event.client_id, event.entity, event.packet);
    }
}

pub struct LoginPlugin;

impl Plugin for LoginPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PreUpdate,
            handle_login_packets.after(ingest_network_packets),
        );
    }
}

fn handle_login_packets(world: &mut World) {
    let packets = std::mem::take(&mut world.resource_mut::<PacketQueue<LoginPacketEvent>>().0);
    for event in packets {
        login::handle_login_packet(world, event.client_id, event.entity, event.packet);
    }
}

pub struct ConfigurationPlugin;

impl Plugin for ConfigurationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PreUpdate,
            handle_configuration_packets.after(ingest_network_packets),
        );
    }
}

fn handle_configuration_packets(world: &mut World) {
    let packets = std::mem::take(
        &mut world
            .resource_mut::<PacketQueue<ConfigurationPacketEvent>>()
            .0,
    );
    for event in packets {
        configuration::handle_configuration_packet(
            world,
            event.client_id,
            event.entity,
            event.packet,
        );
    }
}

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
