use crate::network::{IncomingPacket, OutgoingPacket};
use flume::{Receiver, Sender};
use void_net::socket::ClientSocket;
use void_protocol::clientbound::ClientboundPacket;

pub struct Client {
    socket: ClientSocket,
    incoming_tx: Sender<IncomingPacket>,
    outgoing_rx: Receiver<OutgoingPacket>,
    client_id: u32,
}

impl Client {
    pub fn new(
        id: u32,
        socket: ClientSocket,
        incoming_tx: Sender<IncomingPacket>,
        outgoing_rx: Receiver<OutgoingPacket>,
    ) -> Self {
        Self {
            socket,
            incoming_tx,
            outgoing_rx,
            client_id: id,
        }
    }

    pub async fn run(mut self) -> std::io::Result<()> {
        loop {
            tokio::select! {
                result = self.socket.receive() => {
                    let packet = result?;
                    self.incoming_tx
                        .send(IncomingPacket {
                            client_id: self.client_id,
                            packet: packet,
                        })
                        .expect("Failed to send incoming packet to channel");
                }

                result = self.outgoing_rx.recv_async() => {
                    let outgoing_packet = result.expect("Failed to receive outgoing packet from channel");
                    tracing::debug!("Sending packet to client {}: {:?}", self.client_id, outgoing_packet.packet);
                    match outgoing_packet.packet {
                        ClientboundPacket::Status(packet) => self.socket.send(&packet).await?,
                        ClientboundPacket::Login(packet) => self.socket.send(&packet).await?,
                        ClientboundPacket::Configuration(packet) => self.socket.send(&packet).await?,
                        ClientboundPacket::Play(packet) => self.socket.send(&packet).await?,
                    }
                }
            };
        }
    }
}
