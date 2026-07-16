use crate::network::{IncomingPacket, OutgoingPacket};
use crate::server_status::ServerStatusSnapshot;
use flume::{Receiver, Sender};
use voidmc_net::socket::{ClientSocket, Packet};
use voidmc_protocol::{State, clientbound, serverbound};

enum StatusFastPath {
    // Inspect only the initial handshake; every non-status connection is then
    // permanently handed back to Bevy's normal packet path.
    Handshake,
    Status,
    Disabled,
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

    pub async fn run(mut self) -> std::io::Result<()> {
        loop {
            tokio::select! {
                result = self.socket.receive() => {
                    let packet = result?;
                    if self.handle_status_packet(&packet).await? {
                        continue;
                    }

                    if self.incoming_tx
                        .send(IncomingPacket {
                            client_id: self.client_id,
                            packet,
                        })
                        .is_err()
                    {
                        return Ok(());
                    }
                }

                result = self.outgoing_rx.recv_async() => {
                    let Ok(outgoing_packet) = result else {
                        return Ok(());
                    };
                    match outgoing_packet.packet {
                        clientbound::ClientboundPacket::Status(packet) => self.socket.send(&packet).await?,
                        clientbound::ClientboundPacket::Login(packet) => self.socket.send(&packet).await?,
                        clientbound::ClientboundPacket::Configuration(packet) => self.socket.send(&packet).await?,
                        clientbound::ClientboundPacket::ManualConfiguration(packet) => self.socket.send(&packet).await?,
                        clientbound::ClientboundPacket::Play(packet) => self.socket.send(&packet).await?,
                        clientbound::ClientboundPacket::ManualPlay(packet) => self.socket.send(&packet).await?,
                    }
                }
            };
        }
    }

    async fn handle_status_packet(&mut self, packet: &Packet) -> std::io::Result<bool> {
        match self.status_fast_path {
            StatusFastPath::Handshake => {
                let Ok(serverbound::HandshakePacket::Handshake(handshake)) =
                    packet.decode::<serverbound::HandshakePacket>()
                else {
                    return Ok(false);
                };

                if handshake.next_state == State::Status {
                    self.status_fast_path = StatusFastPath::Status;
                    Ok(true)
                } else {
                    self.status_fast_path = StatusFastPath::Disabled;
                    Ok(false)
                }
            }
            StatusFastPath::Status => {
                // Server-list requests do not mutate the game world, so they can
                // be answered on the Tokio task without waiting for the next tick.
                let packet = packet.decode::<serverbound::StatusPacket>()?;
                let response = match packet {
                    serverbound::StatusPacket::StatusRequest(_) => {
                        clientbound::StatusPacket::StatusResponse(
                            self.server_status
                                .as_ref()
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

                self.socket.send(&response).await?;
                Ok(true)
            }
            StatusFastPath::Disabled => Ok(false),
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
        let socket = ServerSocket(listener).accept().await.unwrap();
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
}
