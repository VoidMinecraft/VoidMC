use serde::{Deserialize, Serialize};
use void_codec::{Decode, Encode};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Version {
    pub name: String,
    pub protocol: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub name: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Players {
    pub max: i32,
    pub online: i32,
    pub sample: Vec<Player>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Description {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    pub version: Version,
    pub players: Players,
    pub description: Description,
    pub favicon: String,
    #[serde(rename = "enforcesSecureChat")]
    pub enforces_secure_chat: bool,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct StatusResponse {
    #[codec(json)]
    pub status: Status,
}
