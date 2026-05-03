use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct SetHeadRotation {
    #[codec(varint32)]
    pub entity_id: i32,
    pub head_yaw: u8,
}
