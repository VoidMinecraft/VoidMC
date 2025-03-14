use crate::codec::{PacketDecode, PacketEncode};
use crate::{Packet, PacketId, State};

#[derive(Debug)]
pub struct Handshake {
    pub protocol_version: i32,
    pub server_address: String,
    pub server_port: u16,
    pub next_state: State,
}

impl Packet for Handshake {
    fn encode<E: PacketEncode>(&self, encoder: &mut E) -> std::io::Result<()> {
        encoder.encode_vari32(self.protocol_version)?;
        encoder.encode_str(&self.server_address)?;
        encoder.encode_u16(self.server_port)?;
        encoder.encode_u8(self.next_state as u8)
    }

    fn decode<D: PacketDecode>(decoder: &mut D) -> std::io::Result<Self> {
        let protocol_version = decoder.decode_vari32()?;
        let server_address = decoder.decode_str()?;
        let server_port = decoder.decode_u16()?;
        let next_state: State = decoder.decode_u8()?.try_into().map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid next_state field in Handshake packet",
            )
        })?;

        Ok(Self {
            protocol_version,
            server_address,
            server_port,
            next_state,
        })
    }
}

impl PacketId for Handshake {
    const ID: i32 = 0x00;
}

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
