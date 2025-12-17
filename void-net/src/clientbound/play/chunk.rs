use crate::codec::{PacketEncode, PacketDecode}; // Adjust imports to match your project
use crate::{Packet, PacketId};
use std::io::{self};
use ussr_nbt::owned::Nbt;

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

impl PacketId for ChunkDataAndLight {
    const ID: i32 = 0x28;
}

impl Packet for ChunkDataAndLight {
    fn encode<E: PacketEncode>(&self, encoder: &mut E) -> io::Result<()> {
        encoder.encode_i32(self.chunk_x)?;
        encoder.encode_i32(self.chunk_z)?;
        encoder.encode_nbt(&self.heightmaps)?;

        let data_len = i32::try_from(self.data.len()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "data length too large for VarInt")
        })?;
        encoder.encode_vari32(data_len)?;
        encoder.write_all(&self.data)?;

        encoder.encode_vari32(0)?; // Block entities count (0)

        // Light Data
        encoder.encode_bitset(&self.sky_light_mask)?;
        encoder.encode_bitset(&self.block_light_mask)?;
        encoder.encode_bitset(&self.empty_sky_light_mask)?;
        encoder.encode_bitset(&self.empty_block_light_mask)?;

        let sky_arrays_len = i32::try_from(self.sky_light_arrays.len()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "sky_light_arrays length too large for VarInt")
        })?;
        encoder.encode_vari32(sky_arrays_len)?;
        for arr in &self.sky_light_arrays {
            let arr_len = i32::try_from(arr.len()).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "sky light array length too large for VarInt")
            })?;
            encoder.encode_vari32(arr_len)?;
            encoder.write_all(arr)?;
        }

        let block_arrays_len = i32::try_from(self.block_light_arrays.len()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "block_light_arrays length too large for VarInt")
        })?;
        encoder.encode_vari32(block_arrays_len)?;
        for arr in &self.block_light_arrays {
            let arr_len = i32::try_from(arr.len()).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "block light array length too large for VarInt")
            })?;
            encoder.encode_vari32(arr_len)?;
            encoder.write_all(arr)?;
        }

        Ok(())
    }

    fn decode<D: PacketDecode>(_: &mut D) -> io::Result<Self> {
        Err(io::Error::new(io::ErrorKind::Other, "decode not implemented"))
    }
}

#[derive(Debug)]
pub struct SetCenterChunk {
    pub chunk_x: i32,
    pub chunk_z: i32,
}

impl PacketId for SetCenterChunk {
    const ID: i32 = 0x58;
}

impl Packet for SetCenterChunk {
    fn encode<E: PacketEncode>(&self, encoder: &mut E) -> io::Result<()> {
        encoder.encode_vari32(self.chunk_x)?;
        encoder.encode_vari32(self.chunk_z)
    }

    fn decode<D: PacketDecode>(_: &mut D) -> io::Result<Self> {
        unimplemented!()
    }
}
