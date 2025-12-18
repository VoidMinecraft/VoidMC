use std::sync::Arc;
use tokio::{
    net::{TcpListener, ToSocketAddrs},
    sync::Mutex,
};
use tracing::{error, info, instrument};

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

#[derive(Debug)]
pub struct Server {
    server: ServerSocket,
    game: Arc<Mutex<Game>>,
}

impl Server {
    #[instrument(level = "info", skip(self))]
    pub async fn run(&self) {
        let local_addr = self.server.0.local_addr().ok();
        if let Some(addr) = local_addr {
            info!(listen_addr = %addr, "Server listening");
        }

        loop {
            match self.server.accept().await {
                Ok(client) => {
                    let game = self.game.clone();
                    let client_ip = client.1.to_string();
                    info!(client_ip = %client_ip, "Accepted new connection");
                    tokio::spawn(async move {
                        if let Err(e) = Client::new(client, game).run().await {
                            info!(client_ip = %client_ip, error = ?e, "Client connection closed");
                        }
                    });
                }
                Err(e) => {
                    error!(error = ?e, "Failed to accept connection");
                }
            };
        }
    }
}
