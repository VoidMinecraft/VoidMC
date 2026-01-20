use uuid::Uuid;
use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct SpawnEntity {
    #[codec(varint32)]
    pub entity_id: i32,
    pub entity_uuid: Uuid,
    #[codec(varint32)]
    pub entity_type: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub pitch: u8,
    pub yaw: u8,
    pub head_yaw: u8,
    #[codec(varint32)]
    pub data: i32,
    pub velocity_x: i16,
    pub velocity_y: i16,
    pub velocity_z: i16,
}
