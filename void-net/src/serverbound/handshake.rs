mod handshake;

pub use handshake::Handshake;

use crate::codec::{PacketDecode, PacketEncode};
use crate::{Packet, PacketId, StatePacket};

#[derive(Debug)]
pub enum HandshakePacket {
    Handshake(Handshake),
}

impl Packet for HandshakePacket {
    fn encode<E: PacketEncode>(&self, encoder: &mut E) -> std::io::Result<()> {
        match self {
            HandshakePacket::Handshake(packet) => {
                encoder.encode_vari32(Handshake::ID)?;
                packet.encode(encoder)
            }
        }
    }

    fn decode<D: PacketDecode>(decoder: &mut D) -> std::io::Result<Self> {
        let id = decoder.decode_vari32()?;

        match id {
            Handshake::ID => Ok(HandshakePacket::Handshake(Handshake::decode(decoder)?)),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Unknown packet ID: {}", id),
            )),
        }
    }
}

impl StatePacket for HandshakePacket {}
