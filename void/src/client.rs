use std::sync::Arc;
use tokio::sync::Mutex;

use crate::game::Game;
use void_net::{ClientSocket, State, serverbound};

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

        Ok(())
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

    async fn run(mut self) -> std::io::Result<()> {
        loop {
            match self.client.receive::<serverbound::HandshakePacket>().await {
                Ok(packet) => match packet {
                    serverbound::HandshakePacket::Handshake(packet) => match packet.next_state {
                        State::Status => {}
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
