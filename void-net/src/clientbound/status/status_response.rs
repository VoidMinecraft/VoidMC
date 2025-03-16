use crate::codec::{PacketDecode, PacketEncode};
use crate::{Packet, PacketId};

use serde::{Deserialize, Serialize};

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
