use voidmc_codec::{Decode, Encode};

use super::TeleportFlags;

#[derive(Debug, Clone, Encode, Decode)]
pub struct TeleportEntity {
    #[codec(varint32)]
    pub entity_id: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub vx: f64,
    pub vy: f64,
    pub vz: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub relatives: TeleportFlags,
    pub on_ground: bool,
}
