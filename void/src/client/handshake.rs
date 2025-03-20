use std::sync::Arc;
use tokio::sync::Mutex;

use super::login::LoginClient;
use super::status::StatusClient;
use crate::game::Game;
use void_net::{ClientSocket, State, serverbound};

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
        println!("[{}] State is now Handshake", socket.1);
        Self { socket, game }
    }

    pub async fn run(mut self) -> std::io::Result<HandshakeClientNext> {
        loop {
            match self.socket.receive::<serverbound::HandshakePacket>().await {
                Ok(packet) => match packet {
                    serverbound::HandshakePacket::Handshake(packet) => match packet.next_state {
                        State::Status => {
                            return Ok(HandshakeClientNext::Status(StatusClient::new(
                                self.socket,
                                self.game,
                                packet.protocol_version,
                            )));
                        }
                        State::Login => {
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
                    eprintln!("Failed to receive packet: {:?}", e);
                }
            }
        }
    }
}
