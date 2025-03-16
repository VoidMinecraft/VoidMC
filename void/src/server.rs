use std::sync::Arc;
use tokio::{
    net::{TcpListener, ToSocketAddrs},
    sync::Mutex,
};

use crate::{client::Client, game::Game};
use void_net::ServerSocket;

pub struct ServerBuilder<A: ToSocketAddrs> {
    addr: A,
    motd: Option<String>,
    favicon: Option<String>,
}

impl<A: ToSocketAddrs> ServerBuilder<A> {
    pub fn new(addr: A) -> Self {
        Self {
            addr,
            motd: None,
            favicon: None,
        }
    }

    pub fn motd(mut self, motd: String) -> Self {
        self.motd = Some(motd);
        self
    }

    pub fn favicon(mut self, favicon: String) -> Self {
        self.favicon = Some(favicon);
        self
    }

    pub async fn build(self) -> std::io::Result<Server> {
        let listener = TcpListener::bind(&self.addr).await?;

        Ok(Server {
            server: ServerSocket(listener),
            game: Arc::new(Mutex::new(Game {
                motd: self.motd.unwrap_or("Void server".to_string()),
                favicon: self.favicon.unwrap_or("".to_string()),
            })),
        })
    }
}

pub struct Server {
    server: ServerSocket,
    game: Arc<Mutex<Game>>,
}

impl Server {
    pub async fn run(&self) -> ! {
        loop {
            match self.server.accept().await {
                Ok(client) => {
                    let game = self.game.clone();
                    tokio::spawn(async move {
                        let _ = Client::new(client, game).run().await;
                    });
                }
                Err(e) => {
                    eprintln!("Failed to accept connection: {}", e);
                }
            };
        }
    }
}
