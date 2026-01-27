mod client;
mod game;
mod server;

pub use server::Server;
use void_net::socket::Packet;

pub struct IncomingPacket {
    pub client_id: u32, // who sent it
    pub packet: Packet,
}

pub struct OutgoingPacket {
    pub client_id: u32, // to whom to send (should be improved for broadcasting)
    pub packet: void_protocol::clientbound::ClientboundPacket,
}
