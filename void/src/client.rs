use crate::network::{IncomingPacket, OutgoingPacket};
use crate::server_status::ServerStatusSnapshot;
use flume::{Receiver, Sender};
use voidmc_net::socket::{ClientSocket, ClientWriter, Packet, SocketError};
use voidmc_protocol::{State, clientbound, serverbound};

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

pub struct Client {
    socket: ClientSocket,
    incoming_tx: Sender<IncomingPacket>,
    outgoing_rx: Receiver<OutgoingPacket>,
    client_id: u32,
    server_status: Option<ServerStatusSnapshot>,
    status_fast_path: StatusFastPath,
}

impl Client {
    pub fn new(
        id: u32,
        socket: ClientSocket,
        incoming_tx: Sender<IncomingPacket>,
        outgoing_rx: Receiver<OutgoingPacket>,
        server_status: Option<ServerStatusSnapshot>,
    ) -> Self {
        let status_fast_path = if server_status.is_some() {
            StatusFastPath::Handshake
        } else {
            StatusFastPath::Disabled
        };

        Self {
            socket,
            incoming_tx,
            outgoing_rx,
            client_id: id,
            server_status,
            status_fast_path,
        }
    }

    pub async fn run(self) -> Result<(), SocketError> {
        let (mut reader, mut writer) = self.socket.into_split();
        let (status_tx, status_rx) = flume::unbounded();
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
                        if status_tx.send(response).is_err() {
                            return Ok(());
                        }
                    }
                    continue;
                }

                if self
                    .incoming_tx
                    .send(IncomingPacket {
                        client_id: self.client_id,
                        packet,
                    })
                    .is_err()
                {
                    return Ok(());
                }
            }
        };

        let writer_loop = Self::write_loop(&mut writer, &self.outgoing_rx, &status_rx);

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
    ) -> Result<(), SocketError> {
        enum Outbound {
            Game(OutgoingPacket),
            Status(clientbound::StatusPacket),
        }

        let mut status_open = true;
        loop {
            let outbound = if status_open {
                tokio::select! {
                    result = outgoing_rx.recv_async() => {
                        let Ok(packet) = result else {
                            return Ok(());
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
                let Ok(packet) = outgoing_rx.recv_async().await else {
                    return Ok(());
                };
                Outbound::Game(packet)
            };

            match outbound {
                Outbound::Status(packet) => writer.send(&packet).await?,
                Outbound::Game(outgoing_packet) => match outgoing_packet.packet {
                    clientbound::ClientboundPacket::Status(packet) => writer.send(&packet).await?,
                    clientbound::ClientboundPacket::Login(packet) => writer.send(&packet).await?,
                    clientbound::ClientboundPacket::Configuration(packet) => {
                        writer.send(&packet).await?
                    }
                    clientbound::ClientboundPacket::ManualConfiguration(packet) => {
                        writer.send(&packet).await?
                    }
                    clientbound::ClientboundPacket::Play(packet) => writer.send(&packet).await?,
                    clientbound::ClientboundPacket::ManualPlay(packet) => {
                        writer.send(&packet).await?
                    }
                },
            }
        }
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
    use voidmc_net::socket::ServerSocket;
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
        let config = ServerConfig {
            max_players: 200,
            motd: "Immediate status".to_string(),
            ..Default::default()
        };
        let status = ServerStatusSnapshot::new(&config);
        status.update(config.max_players, 12, &config.motd);

        let client =
            tokio::spawn(Client::new(7, socket, incoming_tx, outgoing_rx, Some(status)).run());

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
    async fn login_handshake_still_uses_the_bevy_channel() {
        let (socket, mut peer) = connected_socket().await;
        let (incoming_tx, incoming_rx) = flume::unbounded();
        let (_outgoing_tx, outgoing_rx) = flume::unbounded();
        let status = ServerStatusSnapshot::new(&ServerConfig::default());
        let client =
            tokio::spawn(Client::new(7, socket, incoming_tx, outgoing_rx, Some(status)).run());

        send_packet(&mut peer, &handshake(State::Login)).await;

        let incoming = tokio::time::timeout(Duration::from_secs(1), incoming_rx.recv_async())
            .await
            .expect("login handshake should be forwarded")
            .unwrap();
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
        let client = tokio::spawn(Client::new(7, socket, incoming_tx, outgoing_rx, None).run());

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
                assert_eq!(received.client_id, 7);
                assert_eq!(received.packet.decode::<String>().unwrap(), inbound);
            }
        })
        .await
        .expect("bidirectional fragmented traffic should keep making progress");

        drop(peer);
        assert!(client.await.unwrap().is_err());
    }

    fn send_ping(outgoing_tx: &Sender<OutgoingPacket>, timestamp: i64) {
        outgoing_tx
            .send(OutgoingPacket {
                client_id: 7,
                packet: clientbound::ClientboundPacket::Status(
                    clientbound::StatusPacket::PingResponse(clientbound::PingResponse {
                        timestamp,
                    }),
                ),
            })
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
