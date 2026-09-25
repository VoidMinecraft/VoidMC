use std::collections::HashMap;
use std::time::Instant;

use flume::{Receiver, Sender};
use tokio::net::TcpListener;
use tokio::task::{AbortHandle, Id, JoinError, JoinSet};
use tracing::{error, info, instrument};

use crate::{
    client::Client,
    network::{
        CloseRequest, ConnectionCommand, ConnectionEvent, ConnectionHandle, ConnectionId,
        DisconnectReason, OUTBOUND_QUEUE_CAPACITY,
    },
    server_status::ServerStatusSnapshot,
};
use voidmc_net::socket::{FrameLimits, ServerSocket, SocketError};

#[derive(Debug)]
pub struct Server {
    socket: ServerSocket,
    connections: HashMap<ConnectionId, (AbortHandle, Sender<CloseRequest>)>,
    next_id: Option<u32>,
}

impl Server {
    pub async fn new(addr: &str) -> std::io::Result<Self> {
        Self::new_with_limits(addr, FrameLimits::default()).await
    }

    pub async fn new_with_limits(addr: &str, limits: FrameLimits) -> std::io::Result<Self> {
        let server = TcpListener::bind(addr).await?;
        Ok(Self {
            socket: ServerSocket::new(server, limits),
            connections: HashMap::new(),
            next_id: Some(1),
        })
    }

    #[instrument(level = "info", skip(self))]
    pub async fn run(
        &mut self,
        events: Sender<ConnectionEvent>,
        control: Receiver<ConnectionCommand>,
        control_tx: Sender<ConnectionCommand>,
    ) {
        self.run_inner(events, control, control_tx, None).await;
    }

    pub(crate) async fn run_with_status(
        &mut self,
        events: Sender<ConnectionEvent>,
        control: Receiver<ConnectionCommand>,
        control_tx: Sender<ConnectionCommand>,
        server_status: ServerStatusSnapshot,
    ) {
        self.run_inner(events, control, control_tx, Some(server_status))
            .await;
    }

    async fn run_inner(
        &mut self,
        events: Sender<ConnectionEvent>,
        control: Receiver<ConnectionCommand>,
        control_tx: Sender<ConnectionCommand>,
        server_status: Option<ServerStatusSnapshot>,
    ) {
        let local_addr = self.socket.local_addr().ok();
        if let Some(addr) = local_addr {
            info!(listen_addr = %addr, "Server listening");
        }

        let mut tasks = JoinSet::new();
        let mut task_ids = HashMap::new();
        let mut close_deadlines: HashMap<ConnectionId, Instant> = HashMap::new();
        let mut close_reasons: HashMap<ConnectionId, DisconnectReason> = HashMap::new();
        let inbound_quota = crate::network::Quota::new(
            crate::network::MAX_CONNECTIONS * crate::network::INBOUND_QUEUE_CAPACITY,
            crate::network::GLOBAL_INBOUND_BYTES,
        );
        let outbound_quota = crate::network::Quota::new(
            crate::network::MAX_CONNECTIONS * OUTBOUND_QUEUE_CAPACITY,
            crate::network::GLOBAL_OUTBOUND_BYTES,
        );

        loop {
            let next_deadline = close_deadlines.values().min().copied();
            tokio::select! {
                result = self.socket.accept() => {
                    match result {
                        Ok(client) => {
                            if self.connections.len() >= crate::network::MAX_CONNECTIONS {
                                tracing::warn!(capacity = crate::network::MAX_CONNECTIONS, "Connection admission limit reached");
                                continue;
                            }
                            let client_ip = client.peer_addr().to_string();
                            info!(client_ip = %client_ip, "Accepted new connection");

                            let Some(client_id) = self.allocate_id() else {
                                error!("Connection ID space exhausted; rejecting connection");
                                continue;
                            };

                            let task_events = events.clone();
                            let server_status = server_status.clone();
                            let (outgoing_tx, outgoing_rx) = flume::bounded(OUTBOUND_QUEUE_CAPACITY);
                            let (close_tx, close_rx) = flume::bounded(1);
                            let handle = ConnectionHandle::new(client_id, outgoing_tx, control_tx.clone())
                                .with_global_quota(outbound_quota.clone());

                            // All events for this connection share one FIFO stream.
                            if events.send_async(ConnectionEvent::Connected(handle)).await.is_err() {
                                info!("Lifecycle channel closed; shutting down network server");
                                break;
                            }

                            let task = tasks.spawn(
                                Client::new(client_id.0, client, task_events, outgoing_rx, close_rx, server_status)
                                    .with_inbound_quota(inbound_quota.clone()).run(),
                            );
                            task_ids.insert(task.id(), client_id);
                            self.connections.insert(client_id, (task, close_tx));
                        }
                        Err(e) => {
                            error!(error = ?e, "Failed to accept connection");
                        }
                    }
                }

                Some(result) = tasks.join_next_with_id(), if !tasks.is_empty() => {
                    self.finish_task(result, &mut task_ids, &mut close_deadlines, &mut close_reasons, &events).await;
                }

                result = control.recv_async() => {
                    match result {
                        Ok(ConnectionCommand::Close { id, request }) => {
                            if let Some((_, close)) = self.connections.get(&id) {
                                let request = *request;
                                let deadline = request.deadline;
                                let reason = request.reason.clone();
                                if close.try_send(request).is_ok() {
                                    close_reasons.insert(id, reason);
                                    close_deadlines.insert(id, deadline);
                                } else {
                                    tracing::warn!(client_id = id.0, "Duplicate or closed actor close request");
                                }
                            }
                        }
                        Ok(ConnectionCommand::Shutdown) | Err(_) => break,
                    }
                }

                () = async {
                    if let Some(deadline) = next_deadline {
                        tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await;
                    }
                }, if next_deadline.is_some() => {
                    let now = Instant::now();
                    let expired: Vec<_> = close_deadlines.iter()
                        .filter(|(_, deadline)| **deadline <= now)
                        .map(|(id, _)| *id).collect();
                    for id in expired {
                        close_deadlines.remove(&id);
                        if let Some((abort, _)) = self.connections.get(&id) {
                            abort.abort();
                        }
                    }
                }
            }
        }
    }

    fn allocate_id(&mut self) -> Option<ConnectionId> {
        let id = self.next_id?;
        self.next_id = id.checked_add(1);
        Some(ConnectionId(id))
    }

    async fn finish_task(
        &mut self,
        result: Result<(Id, Result<DisconnectReason, SocketError>), JoinError>,
        task_ids: &mut HashMap<Id, ConnectionId>,
        close_deadlines: &mut HashMap<ConnectionId, Instant>,
        close_reasons: &mut HashMap<ConnectionId, DisconnectReason>,
        events: &Sender<ConnectionEvent>,
    ) {
        let (task_id, reason) = match result {
            Ok((task_id, Ok(reason))) => (task_id, reason),
            Ok((task_id, Err(SocketError::PeerClosed))) => (task_id, DisconnectReason::PeerClosed),
            Ok((task_id, Err(error))) => {
                info!(?error, "Client connection closed");
                (task_id, DisconnectReason::Error(error.to_string()))
            }
            Err(error) => {
                info!(?error, "Client connection task failed");
                let reason = if error.is_cancelled() {
                    task_ids
                        .get(&error.id())
                        .and_then(|id| close_reasons.get(id))
                        .cloned()
                        .unwrap_or_else(|| DisconnectReason::Error(error.to_string()))
                } else {
                    DisconnectReason::Error(error.to_string())
                };
                (error.id(), reason)
            }
        };
        if let Some(client_id) = task_ids.remove(&task_id) {
            self.connections.remove(&client_id);
            close_deadlines.remove(&client_id);
            close_reasons.remove(&client_id);
            let _ = events
                .send_async(ConnectionEvent::Disconnected {
                    id: client_id,
                    reason,
                })
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    use super::*;

    #[tokio::test]
    async fn run_exits_on_shutdown_command() {
        let mut server = Server::new("127.0.0.1:0").await.unwrap();
        let (events_tx, _events_rx) = flume::unbounded();
        let (control_tx, control_rx) = flume::unbounded();
        control_tx.send(ConnectionCommand::Shutdown).unwrap();

        tokio::time::timeout(
            Duration::from_millis(100),
            server.run(events_tx, control_rx, control_tx),
        )
        .await
        .expect("server should exit on shutdown");
    }

    #[tokio::test]
    async fn accept_announces_a_bounded_sender_before_any_packet() {
        let mut server = Server::new("127.0.0.1:0").await.unwrap();
        let address = server.socket.local_addr().unwrap();
        let (events_tx, events_rx) = flume::unbounded();
        let (control_tx, control_rx) = flume::unbounded();
        let server_control = control_tx.clone();
        let running =
            tokio::spawn(async move { server.run(events_tx, control_rx, server_control).await });

        let mut peer = TcpStream::connect(address).await.unwrap();
        let connected = tokio::time::timeout(Duration::from_secs(1), events_rx.recv_async())
            .await
            .expect("accept should announce the client")
            .unwrap();
        let ConnectionEvent::Connected(connected) = connected else {
            panic!("expected connected");
        };
        assert_eq!(connected.id, ConnectionId(1));
        assert_eq!(
            connected.outgoing().capacity(),
            Some(OUTBOUND_QUEUE_CAPACITY)
        );

        peer.write_all(&[0x01, 0x00]).await.unwrap();
        let incoming = tokio::time::timeout(Duration::from_secs(1), events_rx.recv_async())
            .await
            .expect("packet should follow the announcement")
            .unwrap();
        let ConnectionEvent::Packet(incoming) = incoming else {
            panic!("expected packet");
        };
        assert_eq!(incoming.client_id, 1);

        assert!(connected.close(CloseRequest {
            reason: DisconnectReason::Server("test".into()),
            packet: None,
            deadline: std::time::Instant::now() + Duration::from_secs(1),
        }));
        assert!(!connected.close(CloseRequest {
            reason: DisconnectReason::Server("duplicate".into()),
            packet: None,
            deadline: std::time::Instant::now() + Duration::from_secs(1),
        }));
        let disconnected = tokio::time::timeout(Duration::from_secs(1), events_rx.recv_async())
            .await
            .expect("close should end the connection")
            .unwrap();
        assert!(matches!(
            disconnected,
            ConnectionEvent::Disconnected {
                id: ConnectionId(1),
                reason: DisconnectReason::Server(_)
            }
        ));

        control_tx.send(ConnectionCommand::Shutdown).unwrap();
        tokio::time::timeout(Duration::from_secs(1), running)
            .await
            .expect("server should exit on shutdown")
            .unwrap();
    }

    #[tokio::test]
    async fn inbound_flood_disconnects_when_per_connection_quota_is_full() {
        let mut server = Server::new("127.0.0.1:0").await.unwrap();
        let address = server.socket.local_addr().unwrap();
        let (events_tx, events_rx) = flume::bounded(128);
        let (control_tx, control_rx) = flume::bounded(2);
        let server_control = control_tx.clone();
        let running = tokio::spawn(async move {
            server.run(events_tx, control_rx, server_control).await;
        });

        let mut peer = TcpStream::connect(address).await.unwrap();
        let ConnectionEvent::Connected(handle) = events_rx.recv_async().await.unwrap() else {
            panic!("expected connected");
        };
        peer.write_all(&[0x01, 0x00].repeat(crate::network::INBOUND_QUEUE_CAPACITY + 1))
            .await
            .unwrap();

        tokio::time::timeout(Duration::from_secs(2), async {
            let mut pending = Vec::new();
            for _ in 0..crate::network::INBOUND_QUEUE_CAPACITY {
                pending.push(events_rx.recv_async().await.unwrap());
                assert!(matches!(pending.last(), Some(ConnectionEvent::Packet(_))));
            }
            assert!(matches!(
                events_rx.recv_async().await.unwrap(),
                ConnectionEvent::Disconnected {
                    reason: DisconnectReason::Overloaded,
                    ..
                }
            ));
        })
        .await
        .expect("flood should close deterministically");

        drop(handle);

        control_tx.send(ConnectionCommand::Shutdown).unwrap();
        running.await.unwrap();
    }

    #[tokio::test]
    async fn ids_stop_at_exhaustion() {
        let mut server = Server::new("127.0.0.1:0").await.unwrap();
        server.next_id = Some(u32::MAX);
        assert_eq!(server.allocate_id(), Some(ConnectionId(u32::MAX)));
        assert_eq!(server.allocate_id(), None);
    }

    #[tokio::test]
    async fn connection_churn_releases_all_handles() {
        let mut server = Server::new("127.0.0.1:0").await.unwrap();
        let address = server.socket.local_addr().unwrap();
        let (events_tx, events_rx) = flume::unbounded();
        let (control_tx, control_rx) = flume::unbounded();
        let server_control = control_tx.clone();
        let running = tokio::spawn(async move {
            server.run(events_tx, control_rx, server_control).await;
            server
        });

        tokio::time::timeout(Duration::from_secs(5), async {
            for expected in 1..=64 {
                let peer = TcpStream::connect(address).await.unwrap();
                let ConnectionEvent::Connected(handle) = events_rx.recv_async().await.unwrap()
                else {
                    panic!("expected connected");
                };
                assert_eq!(handle.id, ConnectionId(expected));
                drop(peer);
                let ConnectionEvent::Disconnected { id, .. } =
                    events_rx.recv_async().await.unwrap()
                else {
                    panic!("expected disconnected");
                };
                assert_eq!(id, ConnectionId(expected));
                assert!(handle.outgoing().is_disconnected());
            }
        })
        .await
        .expect("connection churn should finish");

        control_tx.send(ConnectionCommand::Shutdown).unwrap();
        let server = running.await.unwrap();
        assert!(server.connections.is_empty());
    }

    #[tokio::test]
    async fn close_flushes_disconnect_packet_before_ending() {
        let mut server = Server::new("127.0.0.1:0").await.unwrap();
        let address = server.socket.local_addr().unwrap();
        let (events_tx, events_rx) = flume::unbounded();
        let (control_tx, control_rx) = flume::unbounded();
        let server_control = control_tx.clone();
        let running = tokio::spawn(async move {
            server.run(events_tx, control_rx, server_control).await;
        });
        let mut peer = TcpStream::connect(address).await.unwrap();
        let ConnectionEvent::Connected(handle) = events_rx.recv_async().await.unwrap() else {
            panic!("expected connected");
        };
        let packet: voidmc_protocol::clientbound::ClientboundPacket =
            voidmc_protocol::clientbound::Disconnect {
                reason: crate::messages::text_component("bye", crate::messages::TextColor::Red),
            }
            .into();
        assert!(handle.close(CloseRequest {
            reason: DisconnectReason::Server("bye".into()),
            packet: Some(packet),
            deadline: std::time::Instant::now() + Duration::from_secs(1),
        }));
        tokio::time::timeout(Duration::from_secs(1), async {
            let frame_len = peer.read_u8().await.unwrap();
            assert!(frame_len > 1 && frame_len < 0x80);
            assert_eq!(peer.read_u8().await.unwrap(), 0x20);
            let mut rest = vec![0; frame_len as usize - 1];
            peer.read_exact(&mut rest).await.unwrap();
            let mut byte = [0];
            assert_eq!(
                peer.read(&mut byte).await.unwrap(),
                0,
                "socket should close after flush"
            );
        })
        .await
        .expect("disconnect packet should flush");
        assert!(matches!(
            events_rx.recv_async().await.unwrap(),
            ConnectionEvent::Disconnected {
                id: ConnectionId(1),
                reason: DisconnectReason::Server(_)
            }
        ));
        control_tx.send(ConnectionCommand::Shutdown).unwrap();
        running.await.unwrap();
    }

    #[tokio::test]
    async fn peer_and_server_close_emit_one_disconnection() {
        let mut server = Server::new("127.0.0.1:0").await.unwrap();
        let address = server.socket.local_addr().unwrap();
        let (events_tx, events_rx) = flume::unbounded();
        let (control_tx, control_rx) = flume::unbounded();
        let server_control = control_tx.clone();
        let running = tokio::spawn(async move {
            server.run(events_tx, control_rx, server_control).await;
        });
        let peer = TcpStream::connect(address).await.unwrap();
        let ConnectionEvent::Connected(handle) = events_rx.recv_async().await.unwrap() else {
            panic!("expected connected");
        };
        drop(peer);
        let _ = handle.close(CloseRequest {
            reason: DisconnectReason::Server("simultaneous".into()),
            packet: None,
            deadline: Instant::now() + Duration::from_millis(100),
        });
        assert!(matches!(
            events_rx.recv_async().await.unwrap(),
            ConnectionEvent::Disconnected {
                id: ConnectionId(1),
                ..
            }
        ));
        assert!(
            tokio::time::timeout(Duration::from_millis(20), events_rx.recv_async())
                .await
                .is_err()
        );
        control_tx.send(ConnectionCommand::Shutdown).unwrap();
        running.await.unwrap();
    }

    #[tokio::test]
    async fn task_error_removes_registry_entry() {
        let limits = FrameLimits {
            max_outbound_frame_bytes: 1,
            ..FrameLimits::default()
        };
        let mut server = Server::new_with_limits("127.0.0.1:0", limits)
            .await
            .unwrap();
        let address = server.socket.local_addr().unwrap();
        let (events_tx, events_rx) = flume::unbounded();
        let (control_tx, control_rx) = flume::unbounded();
        let server_control = control_tx.clone();
        let running = tokio::spawn(async move {
            server.run(events_tx, control_rx, server_control).await;
            server
        });
        let _peer = TcpStream::connect(address).await.unwrap();
        let ConnectionEvent::Connected(handle) = events_rx.recv_async().await.unwrap() else {
            panic!("expected connected");
        };
        handle
            .outgoing()
            .send(crate::network::OutgoingPacket::new(
                1,
                voidmc_protocol::clientbound::ClientboundPacket::Status(
                    voidmc_protocol::clientbound::StatusPacket::PingResponse(
                        voidmc_protocol::clientbound::PingResponse { timestamp: 7 },
                    ),
                ),
            ))
            .unwrap();
        let event = tokio::time::timeout(Duration::from_secs(1), events_rx.recv_async())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            event,
            ConnectionEvent::Disconnected {
                id: ConnectionId(1),
                reason: DisconnectReason::Error(_)
            }
        ));
        control_tx.send(ConnectionCommand::Shutdown).unwrap();
        assert!(running.await.unwrap().connections.is_empty());
    }

    #[tokio::test]
    async fn task_panic_reports_disconnection_and_removes_handle() {
        let mut server = Server::new("127.0.0.1:0").await.unwrap();
        let mut tasks = JoinSet::<Result<DisconnectReason, SocketError>>::new();
        let task = tasks.spawn(async { panic!("simulated actor panic") });
        let mut task_ids = HashMap::from([(task.id(), ConnectionId(1))]);
        let (close_tx, _close_rx) = flume::unbounded();
        server.connections.insert(ConnectionId(1), (task, close_tx));
        let (events_tx, events_rx) = flume::unbounded();

        server
            .finish_task(
                tasks.join_next_with_id().await.unwrap(),
                &mut task_ids,
                &mut HashMap::new(),
                &mut HashMap::new(),
                &events_tx,
            )
            .await;

        assert!(server.connections.is_empty());
        assert!(task_ids.is_empty());
        assert!(matches!(
            events_rx.try_recv().unwrap(),
            ConnectionEvent::Disconnected {
                id: ConnectionId(1),
                reason: DisconnectReason::Error(_),
            }
        ));
    }
}
