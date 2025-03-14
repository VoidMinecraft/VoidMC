use serde::{Deserialize, Serialize};

use crate::codec::{PacketDecode, PacketEncode};
use crate::{Packet, PacketId, StatePacket};

#[derive(Debug, Serialize, Deserialize)]
pub struct Version {
    pub name: String,
    pub protocol: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Player {
    pub name: String,
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Players {
    pub max: i32,
    pub online: i32,
    pub sample: Vec<Player>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Description {
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Status {
    pub version: Version,
    pub players: Players,
    pub description: Description,
    pub favicon: String,
    #[serde(rename = "enforcesSecureChat")]
    pub enforces_secure_chat: bool,
}

#[derive(Debug)]
pub struct StatusResponse {
    pub status: Status,
}

impl Packet for StatusResponse {
    fn encode<E: PacketEncode>(&self, encoder: &mut E) -> std::io::Result<()> {
        encoder.encode_str(serde_json::to_string(&self.status)?.as_str())
    }

    fn decode<D: PacketDecode>(decoder: &mut D) -> std::io::Result<Self> {
        let status = serde_json::from_str(&decoder.decode_str()?.as_str()).unwrap();

        Ok(Self { status })
    }
}

impl PacketId for StatusResponse {
    const ID: i32 = 0x00;
}

#[derive(Debug)]
pub struct PingResponse {
    pub timestamp: i64,
}

impl Packet for PingResponse {
    fn encode<E: PacketEncode>(&self, encoder: &mut E) -> std::io::Result<()> {
        encoder.encode_i64(self.timestamp)
    }

    fn decode<D: PacketDecode>(decoder: &mut D) -> std::io::Result<Self> {
        let timestamp = decoder.decode_i64()?;

        Ok(Self { timestamp })
    }
}

impl PacketId for PingResponse {
    const ID: i32 = 0x01;
}

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
