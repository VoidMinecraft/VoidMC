use crate::codec::{PacketDecode, PacketEncode};
use crate::{Packet, PacketId, State, StatePacket};

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

impl StatePacket for HandshakePacket {}

#[derive(Debug)]
pub struct StatusRequest {}

impl Packet for StatusRequest {
    fn encode<E: PacketEncode>(&self, _encoder: &mut E) -> std::io::Result<()> {
        Ok(())
    }

    fn decode<D: PacketDecode>(_decoder: &mut D) -> std::io::Result<Self> {
        Ok(Self {})
    }
}

impl PacketId for StatusRequest {
    const ID: i32 = 0x00;
}

#[derive(Debug)]
pub struct PingRequest {
    pub timestamp: i64,
}

impl Packet for PingRequest {
    fn encode<E: PacketEncode>(&self, encoder: &mut E) -> std::io::Result<()> {
        encoder.encode_i64(self.timestamp)
    }

    fn decode<D: PacketDecode>(decoder: &mut D) -> std::io::Result<Self> {
        let timestamp = decoder.decode_i64()?;

        Ok(Self { timestamp })
    }
}

impl PacketId for PingRequest {
    const ID: i32 = 0x01;
}

#[derive(Debug)]
pub enum StatusPacket {
    StatusRequest(StatusRequest),
    PingRequest(PingRequest),
}

impl Packet for StatusPacket {
    fn encode<E: PacketEncode>(&self, encoder: &mut E) -> std::io::Result<()> {
        match self {
            StatusPacket::StatusRequest(packet) => {
                encoder.encode_vari32(StatusRequest::ID)?;
                packet.encode(encoder)
            }
            StatusPacket::PingRequest(packet) => {
                encoder.encode_vari32(PingRequest::ID)?;
                packet.encode(encoder)
            }
        }
    }

    fn decode<D: PacketDecode>(decoder: &mut D) -> std::io::Result<Self> {
        let id = decoder.decode_vari32()?;

        match id {
            StatusRequest::ID => Ok(StatusPacket::StatusRequest(StatusRequest::decode(decoder)?)),
            PingRequest::ID => Ok(StatusPacket::PingRequest(PingRequest::decode(decoder)?)),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Unknown packet ID: {}", id),
            )),
        }
    }
}

impl StatePacket for StatusPacket {}
