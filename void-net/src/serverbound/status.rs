mod ping_request;
mod status_request;

pub use ping_request::PingRequest;
pub use status_request::StatusRequest;

use crate::codec::{PacketDecode, PacketEncode};
use crate::{Packet, PacketId, StatePacket};

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
