use std::collections::HashMap;

use flume::{Receiver, Sender};
use tokio::net::TcpListener;
use tracing::{error, info, instrument};

use crate::{
    client::Client,
    network::{IncomingPacket, OutgoingPacket},
};
use voidmc_net::socket::ServerSocket;

#[derive(Debug)]
pub struct Server {
    socket: ServerSocket,
    channels: HashMap<u32, Sender<OutgoingPacket>>,
    next_id: u32,
}

impl Server {
    pub async fn new(addr: &str) -> std::io::Result<Self> {
        let server = TcpListener::bind(addr).await?;
        Ok(Self {
            socket: ServerSocket(server),
            channels: HashMap::new(),
            next_id: 1,
        })
    }

    #[instrument(level = "info", skip(self))]
    pub async fn run(
        &mut self,
        incoming_tx: Sender<IncomingPacket>,
        outgoing_rx: Receiver<OutgoingPacket>,
        disconnect_tx: Sender<u32>,
        kick_rx: Receiver<u32>,
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

                            let client_id = self.next_id;
                            self.next_id += 1;

                            let incoming_tx = incoming_tx.clone();
                            let disconnect_tx = disconnect_tx.clone();
                            let (outgoing_tx, outgoing_rx) = flume::unbounded();
                            self.channels.insert(client_id, outgoing_tx);

                            tokio::spawn(async move {
                                if let Err(e) = Client::new(client_id, client, incoming_tx, outgoing_rx)
                                    .run()
                                    .await
                                {
                                    info!(client_ip = %client_ip, error = ?e, "Client connection closed");
                                }
                                let _ = disconnect_tx.send(client_id);
                            });
                        }
                        Err(e) => {
                            error!(error = ?e, "Failed to accept connection");
                        }
                    }
                }

                result = outgoing_rx.recv_async() => {
                    let Ok(outgoing_packet) = result else {
                        info!("Outgoing packet channel closed; shutting down network server");
                        break;
                    };
                    let client_id = outgoing_packet.client_id;

                    // Forward the packet to the appropriate client
                    if let Some(client_tx) = self.channels.get(&client_id) {
                        if let Err(e) = client_tx.send(outgoing_packet) {
                            error!(client_id = client_id, error = ?e, "Failed to send packet to client");
                            self.channels.remove(&client_id);
                        }
                    }
                }

                result = kick_rx.recv_async() => {
                    let Ok(client_id) = result else {
                        info!("Kick channel closed; shutting down network server");
                        break;
                    };

                    // Drop the client's outgoing sender — this causes Client::run()
                    // to exit, which then fires the disconnect notification.
                    if self.channels.remove(&client_id).is_some() {
                        info!(client_id = client_id, "Kicked client (dropped channel)");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn run_exits_when_outgoing_channel_closes() {
        let mut server = Server::new("127.0.0.1:0").await.unwrap();
        let (incoming_tx, _incoming_rx) = flume::unbounded();
        let (outgoing_tx, outgoing_rx) = flume::unbounded();
        let (disconnect_tx, _disconnect_rx) = flume::unbounded();
        let (_kick_tx, kick_rx) = flume::unbounded();
        drop(outgoing_tx);

        tokio::time::timeout(
            Duration::from_millis(100),
            server.run(incoming_tx, outgoing_rx, disconnect_tx, kick_rx),
        )
        .await
        .expect("server should exit when the outgoing channel closes");
    }
}
