use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::debug;

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
        let ip = socket.1.to_string();
        debug!(client_ip = %ip, protocol_version, "Client entered Status state");
        Self {
            socket,
            game,
            protocol_version,
        }
    }

    pub async fn run(mut self) -> std::io::Result<()> {
        let ip = self.socket.1.to_string();
        loop {
            match self.socket.receive::<serverbound::StatusPacket>().await {
                Ok(packet) => match packet {
                    serverbound::StatusPacket::StatusRequest(_) => {
                        debug!(client_ip = %ip, "Responding to status request");
                        self.socket
                            .send(&clientbound::StatusPacket::StatusResponse(StatusResponse {
                                status: self.game.lock().await.status(self.protocol_version),
                            }))
                            .await?;
                    }
                    serverbound::StatusPacket::PingRequest(packet) => {
                        debug!(client_ip = %ip, timestamp = packet.timestamp, "Responding to ping request");
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
                    tracing::error!(client_ip = %ip, error = ?e, "Failed to receive status packet");
                }
            }
        }
    }
}
