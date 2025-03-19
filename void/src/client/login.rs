use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;
use crate::game::Game;
use void_net::{clientbound, serverbound, ClientSocket};
use void_net::clientbound::{LoginSuccess, Property};

pub struct LoginClient {
    client: ClientSocket,
    game: Arc<Mutex<Game>>,
    client_identity: Option<ClientIdentity>,
}

pub struct ClientIdentity {
    pub uuid: Uuid,
    pub name: String,
}

impl LoginClient {
    pub fn new(client: ClientSocket, game: Arc<Mutex<Game>>) -> Self {
        Self {
            client,
            game,
            client_identity: None
        }
    }

    pub async fn run(mut self) -> std::io::Result<()> {
        loop {
            match self.client.receive::<serverbound::LoginPacket>().await {
                Ok(packet) => match packet {
                    serverbound::LoginPacket::LoginStart(packet) => {
                        println!("[Auth] Login start for {} ({})", &packet.uuid, &packet.name);

                        // Set identity
                        self.client_identity = Some(ClientIdentity {
                            uuid: packet.uuid,
                            name: packet.name.clone(),
                        });

                        self.client.send(&clientbound::LoginPacket::LoginSuccess(LoginSuccess {
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
                    serverbound::LoginPacket::LoginAcknowledged(packet) => {
                        let client_identity;
                        match self.client_identity {
                            Some(ref identity) => {
                                client_identity = identity;
                            }
                            None => {
                                eprintln!("[Auth] Login acknowledged without identity");
                                return Err(std::io::Error::new(std::io::ErrorKind::Other, "Login acknowledged without identity"));
                            }
                        }
                        println!("[Auth] Login acknowledged for {} ({})", &client_identity.uuid, &client_identity.name);
                    }
                    _ => {}
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
