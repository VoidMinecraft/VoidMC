use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct SetCenterChunk {
    #[codec(varint32)]
    pub chunk_x: i32,
    #[codec(varint32)]
    pub chunk_z: i32,
}
