use crate::IncomingPacket;
use crossbeam_channel::Sender;
use void_net::ClientSocket;
use void_protocol::serverbound::{HandshakePacket, ServerboundPacket, StatusPacket};

pub struct Client {
    socket: ClientSocket,
    channel: Sender<IncomingPacket>,
}

impl Client {
    pub fn new(client: ClientSocket, channel: Sender<IncomingPacket>) -> Self {
        Self {
            socket: client,
            channel,
        }
    }

    pub async fn run(mut self) -> std::io::Result<()> {
        while let Ok(packet) = self.socket.receive::<HandshakePacket>().await {
            self.channel
                .send(IncomingPacket {
                    client_id: 0, // TODO: Assign proper client ID
                    packet: ServerboundPacket::Handshake(packet),
                })
                .unwrap();
        }

        while let Ok(packet) = self.socket.receive::<StatusPacket>().await {
            self.channel
                .send(IncomingPacket {
                    client_id: 0, // TODO: Assign proper client ID
                    packet: ServerboundPacket::Status(packet),
                })
                .unwrap();
        }

        Ok(())
    }
}
