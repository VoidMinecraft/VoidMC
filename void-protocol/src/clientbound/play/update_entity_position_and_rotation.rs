use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct UpdateEntityPositionAndRotation {
    #[codec(varint32)]
    pub entity_id: i32,
    pub delta_x: i16,
    pub delta_y: i16,
    pub delta_z: i16,
    pub yaw: u8,
    pub pitch: u8,
    pub on_ground: bool,
}
