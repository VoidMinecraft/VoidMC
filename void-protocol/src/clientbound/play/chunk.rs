use ussr_nbt::owned::Nbt;
use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct SetCenterChunk {
    #[codec(varint32)]
    pub chunk_x: i32,
    #[codec(varint32)]
    pub chunk_z: i32,
}

#[derive(Debug)]
pub struct ChunkDataAndLight {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub heightmaps: Nbt,
    pub data: Vec<u8>,
    pub block_entities: Vec<u8>,
    pub sky_light_mask: Vec<u64>,
    pub block_light_mask: Vec<u64>,
    pub empty_sky_light_mask: Vec<u64>,
    pub empty_block_light_mask: Vec<u64>,
    pub sky_light_arrays: Vec<Vec<u8>>,
    pub block_light_arrays: Vec<Vec<u8>>,
}

fn write_varint(buf: &mut Vec<u8>, value: i32) {
    let mut value = value as u32;
    loop {
        if (value & !0x7F) == 0 {
            buf.push(value as u8);
            return;
        }
        buf.push(((value & 0x7F) | 0x80) as u8);
        value >>= 7;
    }
}

fn write_bitset(buf: &mut Vec<u8>, bits: &[u64]) {
    write_varint(buf, bits.len() as i32);
    for &long in bits {
        buf.extend_from_slice(&long.to_be_bytes());
    }
}

impl Encode for ChunkDataAndLight {
    fn encode(&self, buf: &mut Vec<u8>) -> std::io::Result<()> {
        buf.extend_from_slice(&self.chunk_x.to_be_bytes());
        buf.extend_from_slice(&self.chunk_z.to_be_bytes());

        // Heightmaps NBT
        ussr_nbt::encode(&self.heightmaps, buf);

        // Data with length prefix
        write_varint(buf, self.data.len() as i32);
        buf.extend_from_slice(&self.data);

        // Block entities count (0)
        write_varint(buf, 0);

        // Light data
        write_bitset(buf, &self.sky_light_mask);
        write_bitset(buf, &self.block_light_mask);
        write_bitset(buf, &self.empty_sky_light_mask);
        write_bitset(buf, &self.empty_block_light_mask);

        // Sky light arrays
        write_varint(buf, self.sky_light_arrays.len() as i32);
        for arr in &self.sky_light_arrays {
            write_varint(buf, arr.len() as i32);
            buf.extend_from_slice(arr);
        }

        // Block light arrays
        write_varint(buf, self.block_light_arrays.len() as i32);
        for arr in &self.block_light_arrays {
            write_varint(buf, arr.len() as i32);
            buf.extend_from_slice(arr);
        }

        Ok(())
    }
}

impl Decode for ChunkDataAndLight {
    fn decode(_buf: &mut std::io::Cursor<&[u8]>) -> std::io::Result<Self> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "decode not implemented for ChunkDataAndLight",
        ))
    }
}
