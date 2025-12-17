use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;
use void_net::ClientSocket;

use super::configuration::ConfigurationClient;
use crate::game::Game;
use void_protocol::clientbound::{LoginSuccess, Property};
use void_protocol::{clientbound, serverbound};

pub struct LoginClient {
    socket: ClientSocket,
    game: Arc<Mutex<Game>>,
    client_identity: Option<ClientIdentity>,
}

pub struct ClientIdentity {
    pub uuid: Uuid,
    pub name: String,
}

impl LoginClient {
    pub fn new(socket: ClientSocket, game: Arc<Mutex<Game>>) -> Self {
        println!("[{}] State is now Login", socket.1);
        Self {
            socket,
            game,
            client_identity: None,
        }
    }

    pub async fn run(mut self) -> std::io::Result<ConfigurationClient> {
        loop {
            match self.socket.receive::<serverbound::LoginPacket>().await {
                Ok(packet) => match packet {
                    serverbound::LoginPacket::LoginStart(packet) => {
                        println!(
                            "[{}] Login start for {} ({})",
                            self.socket.1, &packet.uuid, &packet.name
                        );

                        // Set identity
                        self.client_identity = Some(ClientIdentity {
                            uuid: packet.uuid,
                            name: packet.name.clone(),
                        });

                        self.socket.send(&clientbound::LoginPacket::LoginSuccess(LoginSuccess {
                            uuid: packet.uuid,
                            username: packet.name,
                            properties: vec![
                                Property {
                                    name: "textures".to_string(),
                                    value: "ewogICJ0aW1lc3RhbXAiIDogMTc0MTA5NzkwNjQ2OCwKICAicHJvZmlsZUlkIiA6ICI3ZmQyZmQyY2I2ZDc0ZGRmYjY0MjZjMzI5Mjk2YWRmOCIsCiAgInByb2ZpbGVOYW1lIiA6ICJkYW5kYW4yNjExIiwKICAidGV4dHVyZXMiIDogewogICAgIlNLSU4iIDogewogICAgICAidXJsIiA6ICJodHRwOi8vdGV4dHVyZXMubWluZWNyYWZ0Lm5ldC90ZXh0dXJlLzc3YzQ2MzAyYWU2MmRhOTI0MDVmMjRmZGJjN2FmZGFhOTc3NzRiMGRkODg5MjBkODk3MjNiYTlmMDhiZWI5MDkiCiAgICB9CiAgfQp9".to_string(),
                                    signature: None,
                                }
                            ],
                        })).await?;
                    }
                    serverbound::LoginPacket::LoginAcknowledged(_) => match self.client_identity {
                        Some(identity) => {
                            println!(
                                "[{}] Login acknowledged for {} ({})",
                                self.socket.1, identity.uuid, identity.name
                            );
                            return Ok(
                                ConfigurationClient::new(self.socket, self.game, identity).await?
                            );
                        }
                        None => {
                            eprintln!("[{}] Login acknowledged without identity", self.socket.1);
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                "Login acknowledged without identity",
                            ));
                        }
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
