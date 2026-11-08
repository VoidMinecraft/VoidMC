use std::time::{SystemTime, UNIX_EPOCH};

use bevy_ecs::prelude::*;
use tracing::instrument;
use voidmc_protocol::clientbound;

use crate::components::{ClientId, KeepAliveState, PlayerReady};
use crate::messages::{TextColor, text_component};
use crate::network::{ClientSenders, NetworkChannels};
use crate::players::Players;

/// How long a client may leave a keep-alive unanswered before it is
/// disconnected. Matches the vanilla server's 30 second timeout.
pub const KEEP_ALIVE_TIMEOUT_MS: i64 = 30_000;

#[derive(Resource)]
pub struct KeepAliveTicker {
    pub ticks_since_last: u32,
    pub interval_ticks: u32,
}

impl Default for KeepAliveTicker {
    fn default() -> Self {
        Self {
            ticks_since_last: 0,
            interval_ticks: 200, // 10 seconds at 20 TPS
        }
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[instrument(level = "info", skip(ticker, players, channels, senders, query))]
pub fn send_keep_alive(
    mut ticker: ResMut<KeepAliveTicker>,
    players: Players,
    channels: Res<NetworkChannels>,
    senders: Option<Res<ClientSenders>>,
    mut query: Query<(Entity, &ClientId, &mut KeepAliveState), With<PlayerReady>>,
) {
    let now = now_millis();

    // A client that has not answered within the timeout is gone: tell it so
    // (in case it is merely stalled) and have the network thread drop the
    // socket, which despawns the player like any other disconnect.
    for (entity, client_id, state) in query.iter() {
        if !state.awaiting_response || now - state.last_sent_id < KEEP_ALIVE_TIMEOUT_MS {
            continue;
        }
        if senders
            .as_deref()
            .and_then(|senders| senders.get(entity))
            .is_some_and(|client| client.kicked())
        {
            continue;
        }

        tracing::warn!(
            client_id = client_id.0,
            unanswered_for_ms = now - state.last_sent_id,
            "Client did not answer keep-alive within the timeout; disconnecting"
        );
        players.send(
            entity,
            clientbound::Disconnect {
                reason: text_component("Timed out", TextColor::Red),
            },
        );
        if let Some(client) = senders.as_deref().and_then(|senders| senders.get(entity)) {
            client.mark_kicked();
        }
        let _ = channels.kick.send(client_id.0);
    }

    ticker.ticks_since_last += 1;

    if ticker.ticks_since_last < ticker.interval_ticks {
        return;
    }

    ticker.ticks_since_last = 0;

    let keep_alive_id = now;

    for (entity, client_id, mut keep_alive_state) in query.iter_mut() {
        if keep_alive_state.awaiting_response {
            tracing::warn!(
                "Client {} did not respond to keep-alive, skipping",
                client_id.0
            );
            continue;
        }

        keep_alive_state.last_sent_id = keep_alive_id;
        keep_alive_state.awaiting_response = true;

        players.send(entity, clientbound::KeepAlive { keep_alive_id });

        tracing::debug!(
            "Sent keep-alive {} to client {}",
            keep_alive_id,
            client_id.0
        );
    }
}

#[cfg(test)]
mod tests {
    use bevy_app::{App, Update};
    use flume::Receiver;
    use voidmc_protocol::clientbound::{ClientboundPacket, PlayPacket};

    use super::*;
    use crate::network::{IncomingPacket, OutgoingPacket};

    struct Harness {
        app: App,
        outgoing: Receiver<OutgoingPacket>,
        kicks: Receiver<u32>,
    }

    fn harness() -> Harness {
        let (incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, kick_rx) = flume::unbounded::<u32>();

        let mut app = App::new();
        app.insert_resource(NetworkChannels {
            incoming: incoming_rx,
            outgoing: outgoing_tx,
            disconnect: disconnect_rx,
            kick: kick_tx,
        })
        .insert_resource(KeepAliveTicker {
            ticks_since_last: 0,
            interval_ticks: u32::MAX,
        })
        .insert_non_send_resource((incoming_tx, disconnect_tx))
        .add_systems(Update, send_keep_alive);

        Harness {
            app,
            outgoing: outgoing_rx,
            kicks: kick_rx,
        }
    }

    fn awaiting_since(app: &mut App, client_id: u32, sent_ms_ago: i64) -> Entity {
        app.world_mut()
            .spawn((
                ClientId(client_id),
                PlayerReady,
                KeepAliveState {
                    last_sent_id: now_millis() - sent_ms_ago,
                    awaiting_response: true,
                    latency: 0,
                },
            ))
            .id()
    }

    #[test]
    fn unanswered_keep_alive_past_the_timeout_disconnects_the_client() {
        let mut h = harness();
        awaiting_since(&mut h.app, 7, KEEP_ALIVE_TIMEOUT_MS + 1_000);

        h.app.update();

        let sent: Vec<OutgoingPacket> = h.outgoing.try_iter().collect();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].client_id, 7);
        assert!(matches!(
            sent[0].packet,
            ClientboundPacket::Play(PlayPacket::Disconnect(_))
        ));
        assert_eq!(h.kicks.try_iter().collect::<Vec<_>>(), vec![7]);
    }

    #[test]
    fn unanswered_keep_alive_within_the_timeout_is_left_alone() {
        let mut h = harness();
        awaiting_since(&mut h.app, 7, KEEP_ALIVE_TIMEOUT_MS / 2);

        h.app.update();

        assert!(h.outgoing.try_iter().next().is_none());
        assert!(h.kicks.try_iter().next().is_none());
    }

    #[test]
    fn answered_keep_alive_is_never_timed_out() {
        let mut h = harness();
        h.app.world_mut().spawn((
            ClientId(7),
            PlayerReady,
            KeepAliveState {
                last_sent_id: now_millis() - 2 * KEEP_ALIVE_TIMEOUT_MS,
                awaiting_response: false,
                latency: 12,
            },
        ));

        h.app.update();

        assert!(h.outgoing.try_iter().next().is_none());
        assert!(h.kicks.try_iter().next().is_none());
    }

    #[test]
    fn timed_out_client_is_kicked_once_while_it_lingers() {
        let (_connected_tx, connected_rx) = flume::unbounded();
        let (direct_tx, direct_rx) = flume::unbounded::<OutgoingPacket>();
        let mut h = harness();
        let entity = awaiting_since(&mut h.app, 7, KEEP_ALIVE_TIMEOUT_MS + 1_000);
        let mut senders = ClientSenders::new(connected_rx);
        senders.register(entity, direct_tx);
        h.app.insert_resource(senders);

        h.app.update();
        h.app.update();
        h.app.update();

        assert_eq!(direct_rx.try_iter().count(), 1);
        assert_eq!(h.kicks.try_iter().collect::<Vec<_>>(), vec![7]);
    }
}
