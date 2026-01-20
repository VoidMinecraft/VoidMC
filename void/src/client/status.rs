use std::sync::Arc;
use tokio::sync::Mutex;

use crate::game::Game;
use void_net::ClientSocket;
use void_protocol::{
    clientbound::{self, PingResponse, StatusResponse},
    serverbound,
};

pub struct StatusClient {
    socket: ClientSocket,
    game: Arc<Mutex<Game>>,
    protocol_version: i32,
}

impl StatusClient {
    pub fn new(socket: ClientSocket, game: Arc<Mutex<Game>>, protocol_version: i32) -> Self {
        println!("[{}] State is now Status", socket.1);
        Self {
            socket,
            game,
            protocol_version,
        }
    }

    pub async fn run(mut self) -> std::io::Result<()> {
        loop {
            match self.socket.receive::<serverbound::StatusPacket>().await {
                Ok(packet) => match packet {
                    serverbound::StatusPacket::StatusRequest(_) => {
                        self.socket
                            .send(&clientbound::StatusPacket::StatusResponse(StatusResponse {
                                status: self.game.lock().await.status(self.protocol_version),
                            }))
                            .await?;
                    }
                    serverbound::StatusPacket::PingRequest(packet) => {
                        self.socket
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
