use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::debug;

use super::login::LoginClient;
use super::status::StatusClient;
use crate::game::Game;
use void_net::ClientSocket;
use void_protocol::{State, serverbound};

pub struct HanshakeClient {
    socket: ClientSocket,
    game: Arc<Mutex<Game>>,
}

pub enum HandshakeClientNext {
    Login(LoginClient),
    Status(StatusClient),
}

impl HanshakeClient {
    pub fn new(socket: ClientSocket, game: Arc<Mutex<Game>>) -> Self {
        let ip = socket.1.to_string();
        debug!(client_ip = %ip, "Client entered Handshake state");
        Self { socket, game }
    }

    pub async fn run(mut self) -> std::io::Result<HandshakeClientNext> {
        let ip = self.socket.1.to_string();
        loop {
            match self.socket.receive::<serverbound::HandshakePacket>().await {
                Ok(packet) => match packet {
                    serverbound::HandshakePacket::Handshake(packet) => match packet.next_state {
                        State::Status => {
                            debug!(client_ip = %ip, protocol = packet.protocol_version, "Client transitioning to Status state");
                            return Ok(HandshakeClientNext::Status(StatusClient::new(
                                self.socket,
                                self.game,
                                packet.protocol_version,
                            )));
                        }
                        State::Login => {
                            debug!(client_ip = %ip, protocol = packet.protocol_version, "Client transitioning to Login state");
                            return Ok(HandshakeClientNext::Login(LoginClient::new(
                                self.socket,
                                self.game,
                            )));
                        }
                        _ => {}
                    },
                },
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::UnexpectedEof {
                        return Err(e);
                    }
                    tracing::error!(client_ip = %ip, error = ?e, "Failed to receive handshake packet");
                }
            }
        }
    }
}
