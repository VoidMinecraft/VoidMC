use crate::network::{
    CloseRequest, ConnectionEvent, DisconnectReason, INBOUND_QUEUE_BYTES, INBOUND_QUEUE_CAPACITY,
    IncomingPacket, OutgoingPacket, Quota,
};
use crate::server_status::ServerStatusSnapshot;
use flume::{Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;
use voidmc_net::socket::{ClientSocket, ClientWriter, Packet, SocketError};
use voidmc_protocol::{State, clientbound, serverbound};

const STATUS_RESPONSE_BYTES: usize = 64 * 1024;

enum StatusFastPath {
    // Inspect only the initial handshake; every non-status connection is then
    // permanently handed back to Bevy's normal packet path.
    Handshake,
    Status,
    Disabled,
}

enum StatusAction {
    Forward,
    Consumed(Option<clientbound::StatusPacket>),
}

enum Outbound {
    Game(OutgoingPacket),
    Status(clientbound::StatusPacket),
}

pub struct Client {
    socket: ClientSocket,
    events: Sender<ConnectionEvent>,
    outgoing_rx: Receiver<OutgoingPacket>,
    close_rx: Receiver<CloseRequest>,
    client_id: u32,
    server_status: Option<ServerStatusSnapshot>,
    status_fast_path: StatusFastPath,
    inbound_local: Arc<Quota>,
    inbound_global: Arc<Quota>,
}

impl Client {
    pub fn new(
        id: u32,
        socket: ClientSocket,
        events: Sender<ConnectionEvent>,
        outgoing_rx: Receiver<OutgoingPacket>,
        close_rx: Receiver<CloseRequest>,
        server_status: Option<ServerStatusSnapshot>,
    ) -> Self {
        let status_fast_path = if server_status.is_some() {
            StatusFastPath::Handshake
        } else {
            StatusFastPath::Disabled
        };

        Self {
            socket,
            events,
            outgoing_rx,
            close_rx,
            client_id: id,
            server_status,
            status_fast_path,
            inbound_local: Quota::new(INBOUND_QUEUE_CAPACITY, INBOUND_QUEUE_BYTES),
            inbound_global: Quota::new(INBOUND_QUEUE_CAPACITY, INBOUND_QUEUE_BYTES),
        }
    }

    pub(crate) fn with_inbound_quota(mut self, global: Arc<Quota>) -> Self {
        self.inbound_global = global;
        self
    }

    pub async fn run(self) -> Result<DisconnectReason, SocketError> {
        let (mut reader, mut writer) = self.socket.into_split();
        let (status_tx, status_rx) = flume::bounded(1);
        let mut status_fast_path = self.status_fast_path;

        let reader_loop = async {
            loop {
                let packet = reader.receive().await?;
                let action = Self::handle_status_packet(
                    &mut status_fast_path,
                    self.server_status.as_ref(),
                    &packet,
                )?;
                if let StatusAction::Consumed(response) = action {
                    if let Some(response) = response {
                        use voidmc_codec::Encode;
                        let mut encoded = Vec::new();
                        response.encode(&mut encoded);
                        if encoded.len() > STATUS_RESPONSE_BYTES {
                            tracing::warn!(
                                client_id = self.client_id,
                                bytes = encoded.len(),
                                limit = STATUS_RESPONSE_BYTES,
                                "Status response exceeds queue byte limit"
                            );
                            return Ok(DisconnectReason::Overloaded);
                        }
                        if status_tx.send_async(response).await.is_err() {
                            return Ok(DisconnectReason::PeerClosed);
                        }
                    }
                    continue;
                }

                let bytes = packet.len();
                let Some(local) = self.inbound_local.reserve(bytes) else {
                    let (peak_packets, peak_bytes) = self.inbound_local.high_water();
                    tracing::warn!(
                        client_id = self.client_id,
                        bytes,
                        peak_packets,
                        peak_bytes,
                        "Inbound quota exceeded"
                    );
                    return Ok(DisconnectReason::Overloaded);
                };
                let Some(global) = self.inbound_global.reserve(bytes) else {
                    let (peak_packets, peak_bytes) = self.inbound_global.high_water();
                    tracing::warn!(
                        client_id = self.client_id,
                        bytes,
                        peak_packets,
                        peak_bytes,
                        "Global inbound quota exceeded"
                    );
                    return Ok(DisconnectReason::Overloaded);
                };
                if self
                    .events
                    .send_async(ConnectionEvent::Packet(IncomingPacket::charged(
                        self.client_id,
                        packet,
                        local,
                        global,
                    )))
                    .await
                    .is_err()
                {
                    return Ok(DisconnectReason::PeerClosed);
                }
            }
        };

        let writer_loop =
            Self::write_loop(&mut writer, &self.outgoing_rx, &status_rx, &self.close_rx);

        tokio::select! {
            result = reader_loop => result,
            result = writer_loop => result,
        }
    }

    fn handle_status_packet(
        status_fast_path: &mut StatusFastPath,
        server_status: Option<&ServerStatusSnapshot>,
        packet: &Packet,
    ) -> Result<StatusAction, SocketError> {
        match status_fast_path {
            StatusFastPath::Handshake => {
                let Ok(serverbound::HandshakePacket::Handshake(handshake)) =
                    packet.decode::<serverbound::HandshakePacket>()
                else {
                    return Ok(StatusAction::Forward);
                };

                if handshake.next_state == State::Status {
                    *status_fast_path = StatusFastPath::Status;
                    Ok(StatusAction::Consumed(None))
                } else {
                    *status_fast_path = StatusFastPath::Disabled;
                    Ok(StatusAction::Forward)
                }
            }
            StatusFastPath::Status => {
                // Server-list requests do not mutate the game world, so they can
                // be answered on the Tokio task without waiting for the next tick.
                let packet = packet.decode::<serverbound::StatusPacket>()?;
                let response = match packet {
                    serverbound::StatusPacket::StatusRequest(_) => {
                        clientbound::StatusPacket::StatusResponse(
                            server_status
                                .expect("status fast path requires a server status snapshot")
                                .response(),
                        )
                    }
                    serverbound::StatusPacket::PingRequest(request) => {
                        clientbound::StatusPacket::PingResponse(clientbound::PingResponse {
                            timestamp: request.timestamp,
                        })
                    }
                };

                Ok(StatusAction::Consumed(Some(response)))
            }
            StatusFastPath::Disabled => Ok(StatusAction::Forward),
        }
    }

    async fn write_loop(
        writer: &mut ClientWriter,
        outgoing_rx: &Receiver<OutgoingPacket>,
        status_rx: &Receiver<clientbound::StatusPacket>,
        close_rx: &Receiver<CloseRequest>,
    ) -> Result<DisconnectReason, SocketError> {
        let mut status_open = true;
        loop {
            let first = if status_open {
                tokio::select! {
                    biased;
                    result = close_rx.recv_async() => {
                        if let Ok(request) = result {
                            return Self::finish_close(writer, outgoing_rx, request).await;
                        }
                        return Ok(DisconnectReason::PeerClosed);
                    }
                    result = outgoing_rx.recv_async() => {
                        let Ok(packet) = result else {
                            return Ok(DisconnectReason::PeerClosed);
                        };
                        Outbound::Game(packet)
                    }
                    result = status_rx.recv_async() => {
                        match result {
                            Ok(packet) => Outbound::Status(packet),
                            Err(_) => {
                                status_open = false;
                                continue;
                            }
                        }
                    }
                }
            } else {
                tokio::select! {
                    biased;
                    result = close_rx.recv_async() => {
                        if let Ok(request) = result {
                            return Self::finish_close(writer, outgoing_rx, request).await;
                        }
                        return Ok(DisconnectReason::PeerClosed);
                    }
                    result = outgoing_rx.recv_async() => {
                        let Ok(packet) = result else { return Ok(DisconnectReason::PeerClosed); };
                        Outbound::Game(packet)
                    }
                }
            };

            let batch = Self::write_batch(writer, first, outgoing_rx, status_open, status_rx);
            if let Err(error) = batch.await {
                let _ = writer.flush().await;
                return Err(error);
            }
            writer.flush().await?;
        }
    }

    async fn finish_close(
        writer: &mut ClientWriter,
        outgoing_rx: &Receiver<OutgoingPacket>,
        request: CloseRequest,
    ) -> Result<DisconnectReason, SocketError> {
        let deadline = tokio::time::Instant::from_std(request.deadline);
        let write = async {
            while let Ok(packet) = outgoing_rx.try_recv() {
                Self::write_outbound(writer, Outbound::Game(packet)).await?;
            }
            if let Some(packet) = request.packet {
                Self::write_packet(writer, packet).await?;
            }
            writer.flush().await
        };
        if Instant::now() >= request.deadline {
            return Ok(request.reason);
        }
        match tokio::time::timeout_at(deadline, write).await {
            Ok(Err(error)) => Err(error),
            Ok(Ok(())) | Err(_) => Ok(request.reason),
        }
    }

    async fn write_batch(
        writer: &mut ClientWriter,
        first: Outbound,
        outgoing_rx: &Receiver<OutgoingPacket>,
        status_open: bool,
        status_rx: &Receiver<clientbound::StatusPacket>,
    ) -> Result<(), SocketError> {
        Self::write_outbound(writer, first).await?;
        for _ in 0..128 {
            let Ok(packet) = outgoing_rx.try_recv() else {
                break;
            };
            Self::write_outbound(writer, Outbound::Game(packet)).await?;
        }
        if status_open {
            for _ in 0..128 {
                let Ok(packet) = status_rx.try_recv() else {
                    break;
                };
                Self::write_outbound(writer, Outbound::Status(packet)).await?;
            }
        }
        Ok(())
    }

    async fn write_outbound(
        writer: &mut ClientWriter,
        outbound: Outbound,
    ) -> Result<(), SocketError> {
        match outbound {
            Outbound::Status(packet) => writer.send(&packet).await?,
            Outbound::Game(mut outgoing_packet) => {
                if let Some(mut frame) = outgoing_packet.take_encoded_frame() {
                    writer.send_preencoded_frame(&mut frame).await?;
                } else {
                    Self::write_packet(writer, outgoing_packet.packet).await?;
                }
            }
        }
        Ok(())
    }

    async fn write_packet(
        writer: &mut ClientWriter,
        packet: clientbound::ClientboundPacket,
    ) -> Result<(), SocketError> {
        match packet {
            clientbound::ClientboundPacket::Status(packet) => writer.send(&packet).await?,
            clientbound::ClientboundPacket::Login(packet) => writer.send(&packet).await?,
            clientbound::ClientboundPacket::Configuration(packet) => writer.send(&packet).await?,
            clientbound::ClientboundPacket::ManualConfiguration(packet) => {
                writer.send(&packet).await?
            }
            clientbound::ClientboundPacket::Play(packet) => writer.send(&packet).await?,
            clientbound::ClientboundPacket::ManualPlay(packet) => writer.send(&packet).await?,
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpStream,
    };
    use voidmc_codec::{Decode, Encode, VarI32};
    use voidmc_net::socket::{FrameError, FrameLimits, ServerSocket};
    use voidmc_protocol::PROTOCOL_VERSION;

    use super::*;
    use crate::ServerConfig;

    async fn connected_socket() -> (ClientSocket, TcpStream) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let peer = TcpStream::connect(address).await.unwrap();
        let socket = ServerSocket::new(listener, Default::default())
            .accept()
            .await
            .unwrap();
        (socket, peer)
    }

    async fn send_packet<T: Encode>(peer: &mut TcpStream, packet: &T) {
        let mut packet_bytes = Vec::new();
        packet.encode(&mut packet_bytes);

        let mut frame = Vec::new();
        VarI32(packet_bytes.len() as i32).encode(&mut frame);
        frame.extend(packet_bytes);
        peer.write_all(&frame).await.unwrap();
    }

    async fn receive_packet<T: Decode>(peer: &mut TcpStream) -> T {
        let mut length_bytes = Vec::new();
        loop {
            let byte = peer.read_u8().await.unwrap();
            length_bytes.push(byte);
            if byte & 0x80 == 0 {
                break;
            }
        }

        let length = VarI32::decode(&mut length_bytes.as_slice()).unwrap().0 as usize;
        let mut packet_bytes = vec![0; length];
        peer.read_exact(&mut packet_bytes).await.unwrap();
        T::decode(&mut packet_bytes.as_slice()).unwrap()
    }

    fn encode_frame<T: Encode>(packet: &T) -> Vec<u8> {
        let mut packet_bytes = Vec::new();
        packet.encode(&mut packet_bytes);

        let mut frame = Vec::new();
        VarI32(packet_bytes.len() as i32).encode(&mut frame);
        frame.extend(packet_bytes);
        frame
    }

    fn handshake(next_state: State) -> serverbound::HandshakePacket {
        serverbound::HandshakePacket::Handshake(serverbound::Handshake {
            protocol_version: PROTOCOL_VERSION,
            server_address: "localhost".to_string(),
            server_port: 25565,
            next_state,
        })
    }

    #[tokio::test]
    async fn status_packets_bypass_the_bevy_channel() {
        let (socket, mut peer) = connected_socket().await;
        let (incoming_tx, incoming_rx) = flume::unbounded();
        let (_outgoing_tx, outgoing_rx) = flume::unbounded();
        let (_close_tx, close_rx) = flume::unbounded();
        let config = ServerConfig {
            max_players: 200,
            motd: "Immediate status".to_string(),
            ..Default::default()
        };
        let status = ServerStatusSnapshot::new(&config);
        status.update(config.max_players, 12, &config.motd);

        let client = tokio::spawn(
            Client::new(7, socket, incoming_tx, outgoing_rx, close_rx, Some(status)).run(),
        );

        send_packet(&mut peer, &handshake(State::Status)).await;
        send_packet(
            &mut peer,
            &serverbound::StatusPacket::StatusRequest(serverbound::StatusRequest {}),
        )
        .await;

        let response = tokio::time::timeout(
            Duration::from_secs(1),
            receive_packet::<clientbound::StatusPacket>(&mut peer),
        )
        .await
        .expect("status response should not wait for a Bevy update");
        let clientbound::StatusPacket::StatusResponse(response) = response else {
            panic!("expected status response");
        };
        assert_eq!(response.status.players.max, 200);
        assert_eq!(response.status.players.online, 12);
        assert_eq!(response.status.description.text, "Immediate status");

        send_packet(
            &mut peer,
            &serverbound::StatusPacket::PingRequest(serverbound::PingRequest { timestamp: 42 }),
        )
        .await;
        let response = receive_packet::<clientbound::StatusPacket>(&mut peer).await;
        let clientbound::StatusPacket::PingResponse(response) = response else {
            panic!("expected ping response");
        };
        assert_eq!(response.timestamp, 42);
        assert!(incoming_rx.is_empty());

        drop(peer);
        client.await.unwrap().unwrap_err();
    }

    #[tokio::test]
    async fn oversized_status_response_closes_without_queueing() {
        let (socket, mut peer) = connected_socket().await;
        let (incoming_tx, _incoming_rx) = flume::bounded(1);
        let (_outgoing_tx, outgoing_rx) = flume::bounded(1);
        let (_close_tx, close_rx) = flume::bounded(1);
        let status = ServerStatusSnapshot::new(&ServerConfig {
            motd: "x".repeat(STATUS_RESPONSE_BYTES),
            ..ServerConfig::default()
        });
        let client = tokio::spawn(
            Client::new(7, socket, incoming_tx, outgoing_rx, close_rx, Some(status)).run(),
        );
        send_packet(&mut peer, &handshake(State::Status)).await;
        send_packet(
            &mut peer,
            &serverbound::StatusPacket::StatusRequest(serverbound::StatusRequest {}),
        )
        .await;
        let reason = tokio::time::timeout(Duration::from_secs(1), client)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(reason, DisconnectReason::Overloaded);
    }

    #[tokio::test]
    async fn cached_outbound_frame_reaches_the_peer() {
        use crate::network::{ConnectionHandle, ConnectionId, OutgoingPacket};
        let (socket, mut peer) = connected_socket().await;
        let (incoming_tx, _incoming_rx) = flume::bounded(1);
        let (outgoing_tx, outgoing_rx) = flume::bounded(1);
        let (control_tx, _control_rx) = flume::bounded(1);
        let (_close_tx, close_rx) = flume::bounded(1);
        let handle = ConnectionHandle::new(ConnectionId(7), outgoing_tx, control_tx);
        let client =
            tokio::spawn(Client::new(7, socket, incoming_tx, outgoing_rx, close_rx, None).run());
        handle
            .try_send(OutgoingPacket::new(
                7,
                clientbound::ClientboundPacket::Status(clientbound::StatusPacket::PingResponse(
                    clientbound::PingResponse { timestamp: 42 },
                )),
            ))
            .unwrap();
        let response = tokio::time::timeout(
            Duration::from_secs(1),
            receive_packet::<clientbound::StatusPacket>(&mut peer),
        )
        .await
        .unwrap();
        let clientbound::StatusPacket::PingResponse(response) = response else {
            panic!("expected ping");
        };
        assert_eq!(response.timestamp, 42);
        drop(peer);
        client.await.unwrap().unwrap_err();
    }

    #[tokio::test]
    async fn login_handshake_still_uses_the_bevy_channel() {
        let (socket, mut peer) = connected_socket().await;
        let (incoming_tx, incoming_rx) = flume::unbounded();
        let (_outgoing_tx, outgoing_rx) = flume::unbounded();
        let (_close_tx, close_rx) = flume::unbounded();
        let status = ServerStatusSnapshot::new(&ServerConfig::default());
        let client = tokio::spawn(
            Client::new(7, socket, incoming_tx, outgoing_rx, close_rx, Some(status)).run(),
        );

        send_packet(&mut peer, &handshake(State::Login)).await;

        let incoming = tokio::time::timeout(Duration::from_secs(1), incoming_rx.recv_async())
            .await
            .expect("login handshake should be forwarded")
            .unwrap();
        let ConnectionEvent::Packet(incoming) = incoming else {
            panic!("expected packet");
        };
        let serverbound::HandshakePacket::Handshake(handshake) = incoming
            .packet
            .decode::<serverbound::HandshakePacket>()
            .unwrap();
        assert_eq!(handshake.next_state, State::Login);

        drop(peer);
        client.await.unwrap().unwrap_err();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn outbound_readiness_preserves_fragmented_inbound_frames() {
        let (socket, mut peer) = connected_socket().await;
        let (incoming_tx, incoming_rx) = flume::unbounded();
        let (outgoing_tx, outgoing_rx) = flume::unbounded();
        let (_close_tx, close_rx) = flume::unbounded();
        let client =
            tokio::spawn(Client::new(7, socket, incoming_tx, outgoing_rx, close_rx, None).run());

        tokio::time::timeout(Duration::from_secs(5), async {
            for round in 0..16 {
                // A 127-byte ASCII string plus its one-byte string length makes a
                // 128-byte payload and therefore a two-byte frame length prefix.
                let inbound = format!("{round:02}{}", "x".repeat(125));
                let frame = encode_frame(&inbound);
                assert_eq!(&frame[..2], &[0x80, 0x01]);

                // Force output to become ready after the first length byte has
                // been consumed. The old select loop dropped the receive future
                // here and restarted framing at the second prefix byte.
                peer.write_all(&frame[..1]).await.unwrap();
                send_ping(&outgoing_tx, round * 10);
                assert_ping(&mut peer, round * 10).await;

                // Fragment the rest of the prefix and body one byte at a time,
                // while continuing to exercise the writer independently.
                for (index, byte) in frame[1..].iter().enumerate() {
                    peer.write_all(std::slice::from_ref(byte)).await.unwrap();
                    if index % 32 == 0 {
                        let timestamp = round * 10 + index as i64 + 1;
                        send_ping(&outgoing_tx, timestamp);
                        assert_ping(&mut peer, timestamp).await;
                    }
                }

                let received = incoming_rx.recv_async().await.unwrap();
                let ConnectionEvent::Packet(received) = received else {
                    panic!("expected packet");
                };
                assert_eq!(received.client_id, 7);
                assert_eq!(received.packet.decode::<String>().unwrap(), inbound);
            }
        })
        .await
        .expect("bidirectional fragmented traffic should keep making progress");

        drop(peer);
        assert!(client.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn frames_accepted_before_an_encode_error_still_reach_the_wire() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut peer = TcpStream::connect(address).await.unwrap();
        let limits = FrameLimits {
            max_outbound_frame_bytes: 16,
            ..FrameLimits::default()
        };
        let socket = ServerSocket::new(listener, limits).accept().await.unwrap();
        let (incoming_tx, _incoming_rx) = flume::unbounded();
        let (outgoing_tx, outgoing_rx) = flume::unbounded();
        let (_close_tx, close_rx) = flume::unbounded();

        send_ping(&outgoing_tx, 1);
        send_ping(&outgoing_tx, 2);
        send_ping(&outgoing_tx, 3);
        outgoing_tx
            .send(OutgoingPacket::new(
                7,
                clientbound::ClientboundPacket::Status(clientbound::StatusPacket::StatusResponse(
                    ServerStatusSnapshot::new(&ServerConfig::default()).response(),
                )),
            ))
            .unwrap();
        send_ping(&outgoing_tx, 4);

        let client = Client::new(7, socket, incoming_tx, outgoing_rx, close_rx, None).run();
        let result = tokio::time::timeout(Duration::from_secs(1), client)
            .await
            .expect("writer loop should stop at the rejected frame");
        assert!(matches!(
            result,
            Err(SocketError::Frame(FrameError::FrameTooLarge {
                limit: 16,
                ..
            }))
        ));

        let mut expected = Vec::new();
        for timestamp in 1..=3 {
            expected.extend(encode_frame(&clientbound::StatusPacket::PingResponse(
                clientbound::PingResponse { timestamp },
            )));
        }
        let mut received = Vec::new();
        peer.read_to_end(&mut received).await.unwrap();
        assert_eq!(received, expected);
    }

    fn send_ping(outgoing_tx: &Sender<OutgoingPacket>, timestamp: i64) {
        outgoing_tx
            .send(OutgoingPacket::new(
                7,
                clientbound::ClientboundPacket::Status(clientbound::StatusPacket::PingResponse(
                    clientbound::PingResponse { timestamp },
                )),
            ))
            .unwrap();
    }

    async fn assert_ping(peer: &mut TcpStream, expected_timestamp: i64) {
        let packet = receive_packet::<clientbound::StatusPacket>(peer).await;
        let clientbound::StatusPacket::PingResponse(response) = packet else {
            panic!("expected ping response");
        };
        assert_eq!(response.timestamp, expected_timestamp);
    }
}
