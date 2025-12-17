use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct SetCenterChunk {
    #[codec(varint32)]
    pub chunk_x: i32,
    #[codec(varint32)]
    pub chunk_z: i32,
}

#[derive(Debug, Encode, Decode)]
pub struct ChunkDataAndLight {
    pub chunk_x: i32,
    pub chunk_z: i32,
    #[codec(nbt)]
    pub heightmaps: simdnbt::owned::Nbt,
    #[codec(prefixed_bytes)]
    pub data: Vec<u8>,
    #[codec(varint32)]
    pub block_entities_count: i32, // For now, simplified - always 0
    pub sky_light_mask: BitSet,
    pub block_light_mask: BitSet,
    pub empty_sky_light_mask: BitSet,
    pub empty_block_light_mask: BitSet,
    pub sky_light_arrays: Vec<LightArray>,
    pub block_light_arrays: Vec<LightArray>,
}

#[derive(Debug, Encode, Decode)]
pub struct BitSet {
    pub data: Vec<u64>,
}

#[derive(Debug, Encode, Decode)]
pub struct LightArray {
    #[codec(prefixed_bytes)]
    pub data: Vec<u8>,
}
