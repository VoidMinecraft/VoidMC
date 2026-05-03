use bevy_app::{App, Plugin};
use bevy_ecs::{observer::On, system::Commands, world::World};
use voidmc_protocol::{
    clientbound::KeepAlive,
    serverbound::{ClientInformation, PlayerLoaded, Pong, TickEnd},
};

use crate::{
    components::{ClientSettings, KeepAliveState, PlayerReady},
    events::PlayerReadyEvent,
    network::PacketEvent,
};

pub struct PlayPlugin;

impl Plugin for PlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(handle_player_loaded);
        app.add_observer(handle_tick_end);
        app.add_observer(handle_pong);
        app.add_observer(handle_keep_alive);
        app.add_observer(handle_client_information);
    }
}

fn handle_player_loaded(event: On<PacketEvent<PlayerLoaded>>, mut commands: Commands) {
    commands.entity(event.entity).insert(PlayerReady);
    commands.trigger(PlayerReadyEvent {
        client_id: event.client_id,
        entity: event.entity,
    });
}

fn handle_tick_end(_event: On<PacketEvent<TickEnd>>) {}

fn handle_pong(_event: On<PacketEvent<Pong>>) {}

fn handle_keep_alive(event: On<PacketEvent<KeepAlive>>, world: &World, mut commands: Commands) {
    if let Some(keep_alive_state) = world.get::<KeepAliveState>(event.entity) {
        if keep_alive_state.last_sent_id == event.packet.keep_alive_id {
            commands.entity(event.entity).insert(KeepAliveState {
                awaiting_response: false,
                ..*keep_alive_state
            });
        }
    }
}

fn handle_client_information(event: On<PacketEvent<ClientInformation>>, mut commands: Commands) {
    commands.entity(event.entity).insert(ClientSettings {
        locale: event.packet.locale.clone(),
        view_distance: event.packet.view_distance,
    });
}
