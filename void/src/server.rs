use flume::{Receiver, Sender};
use tokio::net::TcpListener;
use tracing::{error, info, instrument};

use crate::{IncomingPacket, OutgoingPacket, client::Client};
use void_net::socket::ServerSocket;

#[derive(Debug)]
pub struct Server {
    socket: ServerSocket,
}

impl Server {
    pub async fn new(addr: &str) -> std::io::Result<Self> {
        let server = TcpListener::bind(addr).await?;
        Ok(Self {
            socket: ServerSocket(server),
        })
    }

    #[instrument(level = "info", skip(self))]
    pub async fn run(
        &self,
        incoming_tx: Sender<IncomingPacket>,
        outgoing_rx: Receiver<OutgoingPacket>,
    ) {
        let local_addr = self.socket.0.local_addr().ok();
        if let Some(addr) = local_addr {
            info!(listen_addr = %addr, "Server listening");
        }

        loop {
            match self.socket.accept().await {
                Ok(client) => {
                    let client_ip = client.1.to_string();
                    info!(client_ip = %client_ip, "Accepted new connection");

                    let incoming_tx = incoming_tx.clone();
                    let outgoing_rx = outgoing_rx.clone();
                    tokio::spawn(async move {
                        if let Err(e) = Client::new(client, incoming_tx, outgoing_rx).run().await {
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
