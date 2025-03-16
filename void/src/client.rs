use std::sync::Arc;
use tokio::sync::Mutex;

use crate::game::Game;
use void_net::{
    ClientSocket, State,
    clientbound::{self, PingResponse, StatusResponse},
    serverbound,
};

pub struct Client {
    client: ClientSocket,
    game: Arc<Mutex<Game>>,
}

impl Client {
    pub fn new(client: ClientSocket, game: Arc<Mutex<Game>>) -> Self {
        Self { client, game }
    }

    pub async fn run(self) -> std::io::Result<()> {
        let client = HanshakeClient::new(self.client, self.game);
        let client = client.run().await?;
        client.run().await
    }
}

pub struct HanshakeClient {
    client: ClientSocket,
    game: Arc<Mutex<Game>>,
}

impl HanshakeClient {
    pub fn new(client: ClientSocket, game: Arc<Mutex<Game>>) -> Self {
        Self { client, game }
    }

    async fn run(mut self) -> std::io::Result<StatusClient> {
        loop {
            match self.client.receive::<serverbound::HandshakePacket>().await {
                Ok(packet) => match packet {
                    serverbound::HandshakePacket::Handshake(packet) => match packet.next_state {
                        State::Status => {
                            return Ok(StatusClient::new(
                                self.client,
                                self.game,
                                packet.protocol_version,
                            ));
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

pub struct StatusClient {
    client: ClientSocket,
    game: Arc<Mutex<Game>>,
    protocol_version: i32,
}

impl StatusClient {
    fn new(client: ClientSocket, game: Arc<Mutex<Game>>, protocol_version: i32) -> Self {
        Self {
            client,
            game,
            protocol_version,
        }
    }

    async fn run(mut self) -> std::io::Result<()> {
        loop {
            match self.client.receive::<serverbound::StatusPacket>().await {
                Ok(packet) => match packet {
                    serverbound::StatusPacket::StatusRequest(_) => {
                        self.client
                            .send(&clientbound::StatusPacket::StatusResponse(StatusResponse {
                                status: self.game.lock().await.status(self.protocol_version),
                            }))
                            .await?;
                    }
                    serverbound::StatusPacket::PingRequest(packet) => {
                        self.client
                            .send(&clientbound::StatusPacket::PingResponse(PingResponse {
                                timestamp: packet.timestamp,
                            }))
                            .await?;
                    }
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
