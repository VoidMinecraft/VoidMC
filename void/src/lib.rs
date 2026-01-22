mod client;
mod game;
mod server;

pub use server::Server;

pub struct IncomingPacket {
    pub client_id: u32, // who sent it
    pub packet: void_protocol::serverbound::ServerboundPacket,
}

pub struct OutgoingPacket {
    pub client_id: u32, // to whom to send (should be improved for broadcasting)
    pub packet: void_protocol::clientbound::ClientboundPacket,
}
