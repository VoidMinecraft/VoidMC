use std::sync::Arc;
use tokio::sync::Mutex;

use super::login::ClientIdentity;
use crate::game::Game;
use void_net::ClientSocket;

pub struct PlayClient {
    socket: ClientSocket,
    game: Arc<Mutex<Game>>,
    identity: ClientIdentity,
}

impl PlayClient {
    pub async fn new(
        socket: ClientSocket,
        game: Arc<Mutex<Game>>,
        identity: ClientIdentity,
    ) -> std::io::Result<Self> {
        Ok(Self {
            socket,
            game,
            identity: identity,
        })
    }

    pub async fn run(self) -> std::io::Result<()> {
        Ok(())
    }
}
