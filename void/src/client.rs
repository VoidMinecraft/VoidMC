mod configuration;
mod handshake;
mod login;
mod status;

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::game::Game;
use handshake::{HandshakeClientNext, HanshakeClient};
use void_net::ClientSocket;

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

        match client {
            HandshakeClientNext::Login(client) => {
                let client = client.run().await?;
                client.run().await
            }
            HandshakeClientNext::Status(client) => client.run().await,
        }
    }
}
