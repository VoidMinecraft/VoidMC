use bevy_app::{App, Plugin};
use bevy_ecs::{
    component::Component,
    observer::On,
    query::{Has, With},
    system::{Commands, Query, Res},
};
use voidmc_protocol::{
    MINECRAFT_VERSION, State, clientbound,
    serverbound::{LoginAcknowledged, LoginStart},
};

use crate::{
    components::{Client, ConnectionState, PlayerName, PlayerUuid},
    config::ServerConfigResource,
    network::PacketEvent,
    players::{DisconnectReason, Players},
};

/// Plugin handling the login state of the Minecraft protocol, where clients can authenticate and join the server.
pub struct LoginPlugin;

impl Plugin for LoginPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(handle_login_start);
        app.add_observer(handle_login_acknowledged);
    }
}

/// A connection the server let through login (sent `LoginSuccess`). It holds
/// one of the `max_players` slots from then until it disconnects, through
/// the configuration and play states.
#[derive(Component)]
pub(crate) struct LoginAccepted;

/// Connections holding a slot: every one let through login, plus any in the
/// configuration or play state. Counted at each `LoginStart`; the marker is
/// inserted before the next packet is dispatched, so logins handled in the
/// same tick see each other.
fn slots_taken(connections: &Query<(&ConnectionState, Has<LoginAccepted>), With<Client>>) -> usize {
    connections
        .iter()
        .filter(|(state, accepted)| {
            *accepted || matches!(state.0, State::Configuration | State::Play)
        })
        .count()
}

fn handle_login_start(
    event: On<PacketEvent<LoginStart>>,
    mut commands: Commands,
    players: Players,
    config: Res<ServerConfigResource>,
    connections: Query<(&ConnectionState, Has<LoginAccepted>), With<Client>>,
) {
    // Like vanilla, `max_players` 0 (or below) refuses everyone.
    let max_players = usize::try_from(config.max_players).unwrap_or(0);
    if slots_taken(&connections) >= max_players {
        tracing::info!(
            client_id = event.client_id,
            name = %event.packet.name,
            max_players = config.max_players,
            "Refused login: the server is full"
        );
        players.kick_with(event.entity, DisconnectReason::server_full());
        return;
    }

    commands.entity(event.entity).insert((
        LoginAccepted,
        PlayerName(event.packet.name.clone()),
        PlayerUuid(event.packet.uuid),
    ));

    players.send(
        event.entity,
        clientbound::LoginSuccess {
            uuid: event.packet.uuid,
            username: event.packet.name.clone(),
            properties: vec![],
        },
    );
}

fn handle_login_acknowledged(
    event: On<PacketEvent<LoginAcknowledged>>,
    mut commands: Commands,
    players: Players,
    accepted: Query<(), With<LoginAccepted>>,
) {
    // Only a login the server accepted moves on: a refused client that
    // acknowledges anyway would otherwise reach configuration without a slot.
    if !accepted.contains(event.entity) {
        tracing::debug!(
            client_id = event.client_id,
            "Ignoring LoginAcknowledged from a connection whose login was not accepted"
        );
        return;
    }

    commands
        .entity(event.entity)
        .insert(ConnectionState(State::Configuration));

    players.send(
        event.entity,
        clientbound::KnownPacks {
            known_packs: vec![clientbound::KnownPack {
                namespace: "minecraft".to_string(),
                id: "core".to_string(),
                version: MINECRAFT_VERSION.to_string(),
            }],
        },
    );
}

#[cfg(test)]
mod tests {
    use bevy_ecs::entity::Entity;
    use flume::Receiver;
    use uuid::Uuid;

    use super::*;
    use crate::{
        ServerConfig,
        components::ClientId,
        network::{IncomingPacket, NetworkChannels, OutgoingPacket},
    };

    struct Harness {
        app: App,
        outgoing: Receiver<OutgoingPacket>,
        kicks: Receiver<u32>,
    }

    fn harness(max_players: i32) -> Harness {
        let (_incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, outgoing) = flume::unbounded::<OutgoingPacket>();
        let (_disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, kicks) = flume::unbounded::<u32>();
        let config = ServerConfig {
            max_players,
            ..ServerConfig::default()
        };
        let mut app = App::new();
        app.insert_resource(NetworkChannels {
            incoming: incoming_rx,
            outgoing: outgoing_tx,
            disconnect: disconnect_rx,
            kick: kick_tx,
        })
        .insert_resource(ServerConfigResource::from(&config))
        .add_plugins(LoginPlugin);
        app.insert_non_send_resource((_incoming_tx, _disconnect_tx));
        Harness {
            app,
            outgoing,
            kicks,
        }
    }

    impl Harness {
        fn client(&mut self, client_id: u32, state: State) -> Entity {
            self.app
                .world_mut()
                .spawn((Client, ClientId(client_id), ConnectionState(state)))
                .id()
        }

        /// Dispatches `packet` like `ingest_network_packets` does, flush included.
        fn dispatch<P: Send + Sync + 'static>(
            &mut self,
            client_id: u32,
            entity: Entity,
            packet: P,
        ) {
            let world = self.app.world_mut();
            world.trigger(PacketEvent {
                client_id,
                entity,
                packet,
            });
            world.flush();
        }

        fn log_in(&mut self, client_id: u32, entity: Entity, name: &str) {
            self.dispatch(
                client_id,
                entity,
                LoginStart {
                    name: name.to_string(),
                    uuid: Uuid::from_u128(client_id.into()),
                },
            );
        }

        fn sent(&self) -> Vec<(u32, clientbound::ClientboundPacket)> {
            self.outgoing
                .try_iter()
                .map(|out| (out.client_id, out.packet))
                .collect()
        }
    }

    fn is_success(packet: &clientbound::ClientboundPacket) -> bool {
        matches!(
            packet,
            clientbound::ClientboundPacket::Login(clientbound::LoginPacket::LoginSuccess(_))
        )
    }

    fn is_server_full(packet: &clientbound::ClientboundPacket) -> bool {
        matches!(
            packet,
            clientbound::ClientboundPacket::Login(clientbound::LoginPacket::LoginDisconnect(
                clientbound::LoginDisconnect { reason }
            )) if reason.translate.as_deref() == Some(DisconnectReason::SERVER_FULL)
                && reason.text.is_none()
        )
    }

    #[test]
    fn logins_handled_in_the_same_tick_cannot_share_the_last_slot() {
        let mut h = harness(1);
        let first = h.client(1, State::Login);
        let second = h.client(2, State::Login);

        h.log_in(1, first, "Alice");
        h.log_in(2, second, "Bob");

        let sent = h.sent();
        assert_eq!(sent.len(), 2, "{sent:?}");
        assert_eq!(sent[0].0, 1);
        assert!(is_success(&sent[0].1), "{sent:?}");
        assert_eq!(sent[1].0, 2);
        assert!(is_server_full(&sent[1].1), "{sent:?}");
        assert_eq!(h.kicks.try_iter().collect::<Vec<_>>(), vec![2]);
        assert!(h.app.world().get::<LoginAccepted>(second).is_none());
        assert!(h.app.world().get::<PlayerUuid>(second).is_none());
    }

    #[test]
    fn configuring_and_playing_connections_hold_slots_ready_or_not() {
        let mut h = harness(2);
        h.client(1, State::Configuration);
        h.client(2, State::Play);
        h.client(3, State::Status);
        let late = h.client(4, State::Login);

        h.log_in(4, late, "Carol");

        let sent = h.sent();
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert!(is_server_full(&sent[0].1), "{sent:?}");
    }

    #[test]
    fn zero_max_players_refuses_everyone() {
        let mut h = harness(0);
        let client = h.client(1, State::Login);

        h.log_in(1, client, "Alice");

        let sent = h.sent();
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert!(is_server_full(&sent[0].1), "{sent:?}");
    }

    #[test]
    fn a_refused_client_cannot_acknowledge_its_way_into_configuration() {
        let mut h = harness(0);
        let client = h.client(1, State::Login);

        h.log_in(1, client, "Alice");
        h.dispatch(1, client, LoginAcknowledged {});

        assert_eq!(
            h.app.world().get::<ConnectionState>(client).map(|s| s.0),
            Some(State::Login)
        );
        let sent = h.sent();
        assert_eq!(sent.len(), 1, "only the refusal: {sent:?}");
    }

    #[test]
    fn an_accepted_login_moves_on_to_configuration() {
        let mut h = harness(1);
        let client = h.client(1, State::Login);

        h.log_in(1, client, "Alice");
        h.dispatch(1, client, LoginAcknowledged {});

        assert_eq!(
            h.app.world().get::<ConnectionState>(client).map(|s| s.0),
            Some(State::Configuration)
        );
        assert!(h.kicks.is_empty());
    }
}
