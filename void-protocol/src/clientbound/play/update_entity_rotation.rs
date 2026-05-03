use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct UpdateEntityRotation {
    #[codec(varint32)]
    pub entity_id: i32,
    pub yaw: u8,
    pub pitch: u8,
    pub on_ground: bool,
}
