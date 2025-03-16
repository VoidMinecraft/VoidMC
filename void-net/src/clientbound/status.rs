mod ping_response;
mod status_response;

pub use ping_response::PingResponse;
pub use status_response::{Description, Player, Players, Status, StatusResponse, Version};

use crate::codec::{PacketDecode, PacketEncode};
use crate::{Packet, PacketId, StatePacket};

#[derive(Debug)]
pub enum StatusPacket {
    StatusResponse(StatusResponse),
    PingResponse(PingResponse),
}

impl Packet for StatusPacket {
    fn encode<E: PacketEncode>(&self, encoder: &mut E) -> std::io::Result<()> {
        match self {
            StatusPacket::StatusResponse(packet) => {
                encoder.encode_vari32(StatusResponse::ID)?;
                packet.encode(encoder)
            }
            StatusPacket::PingResponse(packet) => {
                encoder.encode_vari32(PingResponse::ID)?;
                packet.encode(encoder)
            }
        }
    }

    fn decode<D: PacketDecode>(decoder: &mut D) -> std::io::Result<Self> {
        let id = decoder.decode_vari32()?;

        match id {
            StatusResponse::ID => {
                let packet = StatusResponse::decode(decoder)?;
                Ok(StatusPacket::StatusResponse(packet))
            }
            PingResponse::ID => {
                let packet = PingResponse::decode(decoder)?;
                Ok(StatusPacket::PingResponse(packet))
            }
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid packet ID: {}", id),
            )),
        }
    }
}

impl StatePacket for StatusPacket {}
