use std::time::{SystemTime, UNIX_EPOCH};

use bevy_app::{App, Plugin};
use bevy_ecs::{observer::On, system::Commands, world::World};
use voidmc_protocol::serverbound::{ClientInformation, KeepAlive, PlayerLoaded, Pong, TickEnd};

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
                latency: round_trip_millis(keep_alive_state.last_sent_id),
                ..*keep_alive_state
            });
        }
    }
}

fn round_trip_millis(sent_at: i64) -> i32 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    (now - sent_at).clamp(0, i32::MAX as i64) as i32
}

fn handle_client_information(event: On<PacketEvent<ClientInformation>>, mut commands: Commands) {
    commands.entity(event.entity).insert(ClientSettings {
        locale: event.packet.locale.clone(),
        view_distance: event.packet.view_distance,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keep_alive_response_clears_awaiting_response() {
        let mut app = App::new();
        app.add_plugins(PlayPlugin);

        let entity = app
            .world_mut()
            .spawn(KeepAliveState {
                last_sent_id: 42,
                awaiting_response: true,
                latency: 0,
            })
            .id();

        app.world_mut().trigger(PacketEvent {
            client_id: 7,
            entity,
            packet: KeepAlive { keep_alive_id: 42 },
        });
        app.update();

        let state = app.world().get::<KeepAliveState>(entity).unwrap();
        assert!(!state.awaiting_response);
        assert_eq!(state.last_sent_id, 42);
    }

    #[test]
    fn keep_alive_response_measures_round_trip_latency() {
        let mut app = App::new();
        app.add_plugins(PlayPlugin);

        let sent_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
            - 250;
        let entity = app
            .world_mut()
            .spawn(KeepAliveState {
                last_sent_id: sent_at,
                awaiting_response: true,
                latency: 0,
            })
            .id();

        app.world_mut().trigger(PacketEvent {
            client_id: 7,
            entity,
            packet: KeepAlive {
                keep_alive_id: sent_at,
            },
        });
        app.update();

        let latency = app.world().get::<KeepAliveState>(entity).unwrap().latency;
        assert!((250..1250).contains(&latency), "latency {latency}");
        assert_eq!(round_trip_millis(i64::MAX), 0);
        assert_eq!(round_trip_millis(0), i32::MAX);
    }
}
