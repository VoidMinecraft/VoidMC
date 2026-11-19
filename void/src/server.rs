use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use flume::{Receiver, Sender};
use tokio::net::TcpListener;
use tokio::task::AbortHandle;
use tracing::{error, info, instrument};

use crate::{
    client::Client,
    network::{ClientConnected, IncomingPacket, OUTBOUND_QUEUE_CAPACITY},
    observability::Telemetry,
    server_status::ServerStatusSnapshot,
};
use voidmc_net::socket::{FrameLimits, ServerSocket};

#[derive(Debug)]
pub struct Server {
    socket: ServerSocket,
    connections: HashMap<u32, Connection>,
    next_id: u32,
    telemetry: Option<Arc<Telemetry>>,
}

/// A running client task and the signal that closes it gracefully.
#[derive(Debug)]
struct Connection {
    abort: AbortHandle,
    close: Sender<()>,
}

/// How long a kicked connection may take to write what is queued for it (the
/// `Disconnect` with the reason) before it is aborted.
const KICK_FLUSH_TIMEOUT: Duration = Duration::from_secs(2);

impl Server {
    pub async fn new(addr: &str) -> std::io::Result<Self> {
        Self::new_with_limits(addr, FrameLimits::default()).await
    }

    pub async fn new_with_limits(addr: &str, limits: FrameLimits) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        Ok(Self::from_listener(listener, limits))
    }

    pub fn from_listener(listener: TcpListener, limits: FrameLimits) -> Self {
        Self {
            socket: ServerSocket::new(listener, limits),
            connections: HashMap::new(),
            next_id: 1,
            telemetry: None,
        }
    }

    /// See [`ServerSocket::with_compression_threshold`].
    pub fn with_compression_threshold(mut self, threshold: Option<u32>) -> Self {
        self.socket = self.socket.with_compression_threshold(threshold);
        self
    }

    /// Counts connections, connection errors and kicks for `/metrics`.
    pub(crate) fn with_telemetry(mut self, telemetry: Option<Arc<Telemetry>>) -> Self {
        self.telemetry = telemetry;
        self
    }

    #[instrument(level = "info", skip(self))]
    pub async fn run(
        &mut self,
        incoming_tx: Sender<IncomingPacket>,
        connected_tx: Sender<ClientConnected>,
        disconnect_tx: Sender<u32>,
        kick_rx: Receiver<u32>,
    ) {
        self.run_inner(incoming_tx, connected_tx, disconnect_tx, kick_rx, None)
            .await;
    }

    pub(crate) async fn run_with_status(
        &mut self,
        incoming_tx: Sender<IncomingPacket>,
        connected_tx: Sender<ClientConnected>,
        disconnect_tx: Sender<u32>,
        kick_rx: Receiver<u32>,
        server_status: ServerStatusSnapshot,
    ) {
        self.run_inner(
            incoming_tx,
            connected_tx,
            disconnect_tx,
            kick_rx,
            Some(server_status),
        )
        .await;
    }

    async fn run_inner(
        &mut self,
        incoming_tx: Sender<IncomingPacket>,
        connected_tx: Sender<ClientConnected>,
        disconnect_tx: Sender<u32>,
        kick_rx: Receiver<u32>,
        server_status: Option<ServerStatusSnapshot>,
    ) {
        let local_addr = self.socket.local_addr().ok();
        if let Some(addr) = local_addr {
            info!(listen_addr = %addr, "Server listening");
        }

        let (ended_tx, ended_rx) = flume::unbounded::<u32>();

        loop {
            tokio::select! {
                result = self.socket.accept() => {
                    match result {
                        Ok(client) => {
                            let client_ip = client.peer_addr().to_string();
                            info!(client_ip = %client_ip, "Accepted new connection");
                            if let Some(telemetry) = &self.telemetry {
                                telemetry.connection_accepted();
                            }

                            let client_id = self.next_id;
                            self.next_id += 1;

                            let incoming_tx = incoming_tx.clone();
                            let ended_tx = ended_tx.clone();
                            let server_status = server_status.clone();
                            let telemetry = self.telemetry.clone();
                            let (outgoing_tx, outgoing_rx) = flume::bounded(OUTBOUND_QUEUE_CAPACITY);

                            // The game thread must own this sender before the
                            // client task can forward a single packet.
                            if connected_tx.send(ClientConnected { client_id, outgoing: outgoing_tx }).is_err() {
                                info!("Connected channel closed; shutting down network server");
                                break;
                            }

                            let (close_tx, close_rx) = flume::bounded(1);
                            let task = tokio::spawn(
                                Client::new(client_id, client, incoming_tx, outgoing_rx, server_status)
                                    .with_close_signal(close_rx)
                                    .run(),
                            );
                            self.connections.insert(
                                client_id,
                                Connection { abort: task.abort_handle(), close: close_tx },
                            );

                            tokio::spawn(async move {
                                match task.await {
                                    Ok(Ok(())) => {}
                                    Ok(Err(e)) => {
                                        info!(client_ip = %client_ip, error = ?e, "Client connection closed");
                                        if let Some(telemetry) = &telemetry {
                                            telemetry.connection_ended_with(&e);
                                        }
                                    }
                                    Err(_) => {
                                        info!(client_ip = %client_ip, "Client connection aborted");
                                    }
                                }
                                let _ = ended_tx.send(client_id);
                            });
                        }
                        Err(e) => {
                            error!(error = ?e, "Failed to accept connection");
                        }
                    }
                }

                Ok(client_id) = ended_rx.recv_async() => {
                    self.connections.remove(&client_id);
                    let _ = disconnect_tx.send(client_id);
                }

                result = kick_rx.recv_async() => {
                    let Ok(client_id) = result else {
                        info!("Kick channel closed; shutting down network server");
                        break;
                    };

                    if let Some(connection) = self.connections.remove(&client_id) {
                        // Dropping the signal closes the connection once the
                        // packets queued before the kick are written; a client
                        // that does not read them is cut off after a delay.
                        let Connection { abort, close } = connection;
                        drop(close);
                        tokio::spawn(async move {
                            tokio::time::sleep(KICK_FLUSH_TIMEOUT).await;
                            abort.abort();
                        });
                        info!(client_id = client_id, "Kicked client");
                        if let Some(telemetry) = &self.telemetry {
                            telemetry.client_kicked();
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpStream;

    use super::*;

    #[tokio::test]
    async fn run_exits_when_kick_channel_closes() {
        let mut server = Server::new("127.0.0.1:0").await.unwrap();
        let (incoming_tx, _incoming_rx) = flume::unbounded();
        let (connected_tx, _connected_rx) = flume::unbounded();
        let (disconnect_tx, _disconnect_rx) = flume::unbounded();
        let (kick_tx, kick_rx) = flume::unbounded();
        drop(kick_tx);

        tokio::time::timeout(
            Duration::from_millis(100),
            server.run(incoming_tx, connected_tx, disconnect_tx, kick_rx),
        )
        .await
        .expect("server should exit when the kick channel closes");
    }

    #[tokio::test]
    async fn accept_announces_a_bounded_sender_before_any_packet() {
        let mut server = Server::new("127.0.0.1:0").await.unwrap();
        let address = server.socket.local_addr().unwrap();
        let (incoming_tx, incoming_rx) = flume::unbounded();
        let (connected_tx, connected_rx) = flume::unbounded();
        let (disconnect_tx, disconnect_rx) = flume::unbounded();
        let (kick_tx, kick_rx) = flume::unbounded();
        let running = tokio::spawn(async move {
            server
                .run(incoming_tx, connected_tx, disconnect_tx, kick_rx)
                .await
        });

        let mut peer = TcpStream::connect(address).await.unwrap();
        let connected = tokio::time::timeout(Duration::from_secs(1), connected_rx.recv_async())
            .await
            .expect("accept should announce the client")
            .unwrap();
        assert_eq!(connected.client_id, 1);
        assert_eq!(connected.outgoing.capacity(), Some(OUTBOUND_QUEUE_CAPACITY));

        peer.write_all(&[0x01, 0x00]).await.unwrap();
        let incoming = tokio::time::timeout(Duration::from_secs(1), incoming_rx.recv_async())
            .await
            .expect("packet should follow the announcement")
            .unwrap();
        assert_eq!(incoming.client_id, 1);

        kick_tx.send(1).unwrap();
        let disconnected = tokio::time::timeout(Duration::from_secs(1), disconnect_rx.recv_async())
            .await
            .expect("kick should end the connection")
            .unwrap();
        assert_eq!(disconnected, 1);
        assert!(connected.outgoing.is_disconnected());

        drop(kick_tx);
        tokio::time::timeout(Duration::from_secs(1), running)
            .await
            .expect("server should exit when the kick channel closes")
            .unwrap();
    }

    #[tokio::test]
    async fn telemetry_counts_accepts_framing_errors_and_kicks() {
        let telemetry = Arc::new(Telemetry::new(Duration::from_secs(1), 20, 100));
        let mut server = Server::new("127.0.0.1:0")
            .await
            .unwrap()
            .with_telemetry(Some(telemetry.clone()));
        let address = server.socket.local_addr().unwrap();
        let (incoming_tx, _incoming_rx) = flume::unbounded();
        let (connected_tx, connected_rx) = flume::unbounded();
        let (disconnect_tx, disconnect_rx) = flume::unbounded();
        let (kick_tx, kick_rx) = flume::unbounded();
        let running = tokio::spawn(async move {
            server
                .run(incoming_tx, connected_tx, disconnect_tx, kick_rx)
                .await
        });
        let ended = || async {
            tokio::time::timeout(Duration::from_secs(1), disconnect_rx.recv_async())
                .await
                .expect("the connection should end")
                .unwrap()
        };

        // An empty frame is a framing error.
        let mut bad = TcpStream::connect(address).await.unwrap();
        bad.write_all(&[0x00]).await.unwrap();
        assert_eq!(ended().await, 1);

        // A client that closes is not an error.
        let polite = TcpStream::connect(address).await.unwrap();
        drop(polite);
        assert_eq!(ended().await, 2);

        let _kicked = TcpStream::connect(address).await.unwrap();
        let mut last = None;
        for _ in 0..3 {
            last = Some(
                tokio::time::timeout(Duration::from_secs(1), connected_rx.recv_async())
                    .await
                    .expect("accept should announce the client")
                    .unwrap(),
            );
        }
        kick_tx.send(last.unwrap().client_id).unwrap();
        assert_eq!(ended().await, 3);

        let text = telemetry.render_metrics(std::time::Instant::now());
        for line in [
            "voidmc_connections_accepted_total 3\n",
            "voidmc_connection_errors_total{kind=\"frame\"} 1\n",
            "voidmc_connection_errors_total{kind=\"io\"} 0\n",
            "voidmc_connection_errors_total{kind=\"decode\"} 0\n",
            "voidmc_clients_kicked_total 1\n",
        ] {
            assert!(text.contains(line), "missing {line:?} in\n{text}");
        }

        drop(kick_tx);
        tokio::time::timeout(Duration::from_secs(1), running)
            .await
            .expect("server should exit when the kick channel closes")
            .unwrap();
    }
}
