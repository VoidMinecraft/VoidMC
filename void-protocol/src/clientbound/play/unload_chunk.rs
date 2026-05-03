use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct UnloadChunk {
    pub chunk_z: i32, // Z first per 1.21.4 protocol
    pub chunk_x: i32,
}
