use std::sync::Arc;
use tokio::sync::Mutex;

use crate::game::Game;
use void_net::ClientSocket;

pub struct LoginClient {
    client: ClientSocket,
    game: Arc<Mutex<Game>>,
}

impl LoginClient {
    pub fn new(client: ClientSocket, game: Arc<Mutex<Game>>) -> Self {
        Self { client, game }
    }

    pub async fn run(self) -> std::io::Result<()> {
        todo!()
    }
}
