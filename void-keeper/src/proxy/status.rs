//! Status pings: the client's, relayed or answered, and the proxy's own probes of the server.

use std::net::SocketAddr;
use std::time::Duration;

use bytes::Bytes;
use serde_json::json;
use tokio::io::copy_bidirectional;
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio::time::{sleep, timeout};
use voidmc_codec::{Encode, VarI32};
use voidmc_data::v26_1_2::packets::status::clientbound as status_cb;
use voidmc_data::v26_1_2::packets::status::serverbound as status_sb;
use voidmc_protocol::{MINECRAFT_VERSION, PROTOCOL_VERSION};

use super::Events;
use super::peer::{CONNECT_TIMEOUT, Peer, connect};
use crate::frame;

const STATUS_TIMEOUT: Duration = Duration::from_secs(10);
const PROBE_EVERY: Duration = Duration::from_millis(500);

fn status_response(body: &mut Vec<u8>) {
    let status = json!({
        "version": { "name": MINECRAFT_VERSION, "protocol": PROTOCOL_VERSION },
        "players": { "max": 0, "online": 0 },
        "description": { "text": "The server is away, the keeper is waiting for it" },
    });
    status.to_string().encode(body);
}

async fn relay_for(limit: Duration, client: &mut TcpStream, backend: &mut TcpStream) {
    let _ = timeout(limit, copy_bidirectional(client, backend)).await;
}

/// The server answers its own pings; while it is away, the keeper does.
pub(super) async fn relay_status(mut client: Peer, handshake: Bytes, backend_address: SocketAddr) {
    if let Some(stream) = connect(backend_address).await {
        let mut backend = Peer::new(stream);
        backend.queue(&handshake);
        backend.outbound.extend_from_slice(&client.inbound);
        if backend.flush().await.is_ok() {
            relay_for(STATUS_TIMEOUT, &mut client.stream, &mut backend.stream).await;
        }
        return;
    }
    while let Ok(Ok(Some(request))) = timeout(STATUS_TIMEOUT, client.next()).await {
        match frame::id_and_body(&request) {
            Some((status_sb::STATUS_REQUEST, _)) => {
                client.queue(&frame::build(status_cb::STATUS_RESPONSE, status_response));
            }
            Some((status_sb::PING_REQUEST, body)) => {
                client.queue(&frame::build(status_cb::PONG_RESPONSE, |out| {
                    out.extend_from_slice(body);
                }));
            }
            _ => return,
        }
        if client.flush().await.is_err() {
            return;
        }
    }
}

/// Answers whether a server is up: a status request, which logs nobody in.
pub async fn probe(address: SocketAddr) -> bool {
    let attempt = async {
        let mut peer = Peer::new(connect(address).await?);
        let port = address.port();
        peer.queue(&frame::build(0, |body| {
            VarI32(PROTOCOL_VERSION).encode(body);
            address.ip().to_string().encode(body);
            port.encode(body);
            VarI32(1).encode(body);
        }));
        peer.queue(&frame::build(status_sb::STATUS_REQUEST, |_| {}));
        peer.flush().await.ok()?;
        peer.next().await.ok()?
    };
    timeout(CONNECT_TIMEOUT, attempt)
        .await
        .is_ok_and(|answer| answer.is_some())
}

/// Probes the server while no session is open: a session says itself when it is lost.
pub(super) async fn watch_backend(
    address: SocketAddr,
    events: Events,
    mut open: watch::Receiver<usize>,
) {
    let mut up = None;
    loop {
        if open.wait_for(|count| *count == 0).await.is_err() {
            return;
        }
        let now_up = probe(address).await;
        if up != Some(now_up) {
            up = Some(now_up);
            let name = if now_up { "backend_up" } else { "backend_down" };
            events.emit(name, json!({ "backend": address.to_string() }));
        }
        sleep(PROBE_EVERY).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::peer::pair;

    #[tokio::test]
    async fn a_status_relay_ends_on_its_own() {
        let (mut client, _held) = pair().await;
        let (mut backend, _also_held) = pair().await;
        let relay = relay_for(Duration::from_millis(50), &mut client, &mut backend);
        timeout(Duration::from_secs(5), relay).await.unwrap();
    }
}
