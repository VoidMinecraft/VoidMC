use std::collections::HashMap;

use flume::{Receiver, Sender};
use tokio::net::TcpListener;
use tracing::{error, info, instrument};

use crate::{IncomingPacket, OutgoingPacket, client::Client};
use void_net::socket::ServerSocket;

#[derive(Debug)]
pub struct Server {
    socket: ServerSocket,
    channels: HashMap<u32, Sender<OutgoingPacket>>,
}

impl Server {
    pub async fn new(addr: &str) -> std::io::Result<Self> {
        let server = TcpListener::bind(addr).await?;
        Ok(Self {
            socket: ServerSocket(server),
            channels: HashMap::new(),
        })
    }

    #[instrument(level = "info", skip(self))]
    pub async fn run(
        &mut self,
        incoming_tx: Sender<IncomingPacket>,
        outgoing_rx: Receiver<OutgoingPacket>,
    ) {
        let local_addr = self.socket.0.local_addr().ok();
        if let Some(addr) = local_addr {
            info!(listen_addr = %addr, "Server listening");
        }

        loop {
            tokio::select! {
                result = self.socket.accept() => {
                    match result {
                        Ok(client) => {
                            let client_ip = client.1.to_string();
                            info!(client_ip = %client_ip, "Accepted new connection");

                            // TODO: better client ID management, this can have collisions
                            let client_id = rand::random();
                            let incoming_tx = incoming_tx.clone();
                            let (outgoing_tx, outgoing_rx) = flume::unbounded();
                            self.channels.insert(client_id, outgoing_tx);

                            tokio::spawn(async move {
                                if let Err(e) = Client::new(client_id, client, incoming_tx, outgoing_rx)
                                    .run()
                                    .await
                                {
                                    info!(client_ip = %client_ip, error = ?e, "Client connection closed");
                                }
                            });
                        }
                        Err(e) => {
                            error!(error = ?e, "Failed to accept connection");
                        }
                    }
                }

                result = outgoing_rx.recv_async() => {
                    let outgoing_packet = result.expect("Failed to receive outgoing packet from channel");
                    let client_id = outgoing_packet.client_id;

                    // Forward the packet to the appropriate client
                    if let Some(client_tx) = self.channels.get(&client_id) {
                        if let Err(e) = client_tx.send(outgoing_packet) {
                            error!(client_id = client_id, error = ?e, "Failed to send packet to client");
                        }
                    } else {
                        error!(client_id = client_id, "No channel found for client");
                    }
                }
            }
        }
    }
}
