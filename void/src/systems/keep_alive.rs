use std::time::{SystemTime, UNIX_EPOCH};

use bevy_ecs::prelude::*;
use void_protocol::clientbound;

use crate::components::{ClientId, KeepAliveState, PlayerReady};
use crate::network::{NetworkChannels, OutgoingPacket};

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

pub fn send_keep_alive(
    mut ticker: ResMut<KeepAliveTicker>,
    channels: Res<NetworkChannels>,
    mut query: Query<(&ClientId, &mut KeepAliveState), With<PlayerReady>>,
) {
    ticker.ticks_since_last += 1;

    if ticker.ticks_since_last < ticker.interval_ticks {
        return;
    }

    ticker.ticks_since_last = 0;

    let keep_alive_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    for (client_id, mut keep_alive_state) in query.iter_mut() {
        if keep_alive_state.awaiting_response {
            tracing::warn!(
                "Client {} did not respond to keep-alive, skipping",
                client_id.0
            );
            continue;
        }

        keep_alive_state.last_sent_id = keep_alive_id;
        keep_alive_state.awaiting_response = true;

        let _ = channels.outgoing.send(OutgoingPacket {
            client_id: client_id.0,
            packet: clientbound::ClientboundPacket::Play(clientbound::PlayPacket::KeepAlive(
                clientbound::KeepAlive { keep_alive_id },
            )),
        });

        tracing::debug!(
            "Sent keep-alive {} to client {}",
            keep_alive_id,
            client_id.0
        );
    }
}
