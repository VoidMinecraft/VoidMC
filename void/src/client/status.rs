use std::sync::Arc;
use tokio::sync::Mutex;

use crate::game::Game;
use void_net::{
    ClientSocket,
    clientbound::{self, PingResponse, StatusResponse},
    serverbound,
};

pub struct StatusClient {
    client: ClientSocket,
    game: Arc<Mutex<Game>>,
    protocol_version: i32,
}

impl StatusClient {
    pub fn new(client: ClientSocket, game: Arc<Mutex<Game>>, protocol_version: i32) -> Self {
        Self {
            client,
            game,
            protocol_version,
        }
    }

    pub async fn run(mut self) -> std::io::Result<()> {
        loop {
            match self.client.receive::<serverbound::StatusPacket>().await {
                Ok(packet) => match packet {
                    serverbound::StatusPacket::StatusRequest(_) => {
                        self.client
                            .send(&clientbound::PlayPacket::StatusResponse(StatusResponse {
                                status: self.game.lock().await.status(self.protocol_version),
                            }))
                            .await?;
                    }
                    serverbound::StatusPacket::PingRequest(packet) => {
                        self.client
                            .send(&clientbound::PlayPacket::PingResponse(PingResponse {
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
