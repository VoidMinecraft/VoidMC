use crate::codec::{PacketDecode, PacketEncode};
use crate::{Packet, PacketId};
use std::io;
use ussr_nbt::owned::Nbt;

// ============================================================================
// Chunk Heightmaps
// ============================================================================

/// Represents the heightmap data for a chunk.
/// Contains packed long arrays for different heightmap types.
#[derive(Debug, Clone, Default)]
pub struct ChunkHeightmaps {
    /// MOTION_BLOCKING heightmap: 256 values packed into 37 longs (9 bits per value)
    /// Stores the highest block that blocks motion or contains a fluid
    pub motion_blocking: Vec<i64>,
}

impl ChunkHeightmaps {
    /// Creates empty heightmaps (all zeros)
    pub fn empty() -> Self {
        Self {
            motion_blocking: vec![0i64; 37],
        }
    }

    /// Creates heightmaps with a flat surface at the given Y level
    pub fn flat(surface_y: i32) -> Self {
        let mut motion_blocking = vec![0i64; 37];
        // Pack 256 values (9 bits each) into longs
        // Each long holds floor(64/9) = 7 values
        let value = (surface_y + 64) as u64; // Offset by min_y (-64)
        for i in 0..256 {
            let long_idx = (i * 9) / 64;
            let bit_offset = (i * 9) % 64;
            if long_idx < 37 {
                motion_blocking[long_idx] |= ((value & 0x1FF) << bit_offset) as i64;
                // Handle overflow to next long
                if bit_offset > 55 && long_idx + 1 < 37 {
                    motion_blocking[long_idx + 1] |= ((value & 0x1FF) >> (64 - bit_offset)) as i64;
                }
            }
        }
        Self { motion_blocking }
    }

    /// Converts to NBT format for packet encoding
    pub fn to_nbt(&self) -> Nbt {
        Nbt {
            name: "".into(),
            compound: vec![("MOTION_BLOCKING".into(), self.motion_blocking.clone().into())].into(),
        }
    }
}

// ============================================================================
// Chunk Section
// ============================================================================

/// Represents a single 16x16x16 section of a chunk.
#[derive(Debug, Clone)]
pub struct ChunkSection {
    /// Number of non-air blocks in this section (0-4096)
    pub block_count: i16,
    /// Block state palette entry (for single-value palette) or full palette data
    pub block_state: PaletteData,
    /// Biome palette entry (for single-value palette) or full palette data
    pub biome: PaletteData,
}

/// Palette data for blocks or biomes
#[derive(Debug, Clone)]
pub enum PaletteData {
    /// Single value palette (bits per entry = 0)
    SingleValue(i32),
    // TODO: Add support for indirect and direct palettes
    // Indirect { bits_per_entry: u8, palette: Vec<i32>, data: Vec<u64> },
    // Direct { bits_per_entry: u8, data: Vec<u64> },
}

impl ChunkSection {
    /// Creates an empty section (all air, plains biome)
    pub fn empty() -> Self {
        Self {
            block_count: 0,
            block_state: PaletteData::SingleValue(0), // Air
            biome: PaletteData::SingleValue(1),       // Plains
        }
    }

    /// Creates a section filled with a single block type
    pub fn filled(block_state_id: i32, biome_id: i32) -> Self {
        Self {
            block_count: if block_state_id == 0 { 0 } else { 4096 },
            block_state: PaletteData::SingleValue(block_state_id),
            biome: PaletteData::SingleValue(biome_id),
        }
    }

    /// Encodes this section to bytes
    pub fn encode_to_bytes(&self) -> Vec<u8> {
        let mut data = Vec::new();

        // Block count (i16 big-endian)
        data.extend_from_slice(&self.block_count.to_be_bytes());

        // Block states
        match &self.block_state {
            PaletteData::SingleValue(id) => {
                data.push(0); // bits_per_entry = 0
                write_varint(&mut data, *id);
                write_varint(&mut data, 0); // data array length = 0
            }
        }

        // Biomes
        match &self.biome {
            PaletteData::SingleValue(id) => {
                data.push(0); // bits_per_entry = 0
                write_varint(&mut data, *id);
                write_varint(&mut data, 0); // data array length = 0
            }
        }

        data
    }
}

// ============================================================================
// Light Data
// ============================================================================

/// Represents light data for a chunk (sky light and block light).
#[derive(Debug, Clone, Default)]
pub struct LightData {
    /// Bitmask of sections that have sky light data
    pub sky_light_mask: Vec<u64>,
    /// Bitmask of sections that have block light data
    pub block_light_mask: Vec<u64>,
    /// Bitmask of sections with empty sky light (all zeros)
    pub empty_sky_light_mask: Vec<u64>,
    /// Bitmask of sections with empty block light (all zeros)
    pub empty_block_light_mask: Vec<u64>,
    /// Sky light arrays (2048 bytes each, 4 bits per block)
    pub sky_light_arrays: Vec<Vec<u8>>,
    /// Block light arrays (2048 bytes each, 4 bits per block)
    pub block_light_arrays: Vec<Vec<u8>>,
}

impl LightData {
    /// Creates empty light data (all sections marked as empty)
    pub fn empty() -> Self {
        Self {
            sky_light_mask: vec![0],
            block_light_mask: vec![0],
            empty_sky_light_mask: vec![0x3FFFFFF], // 26 bits set (24 sections + 2 sentinels)
            empty_block_light_mask: vec![0x3FFFFFF],
            sky_light_arrays: Vec::new(),
            block_light_arrays: Vec::new(),
        }
    }

    /// Creates light data with full sky light for all sections
    pub fn full_sky_light() -> Self {
        let num_sections = 26; // 24 sections + 2 sentinels
        let mut sky_light_arrays = Vec::with_capacity(num_sections);
        for _ in 0..num_sections {
            sky_light_arrays.push(vec![0xFF; 2048]); // Full light (15) for all blocks
        }

        Self {
            sky_light_mask: vec![0x3FFFFFF],        // All 26 bits set
            block_light_mask: vec![0],              // No block light
            empty_sky_light_mask: vec![0],          // No empty sky light sections
            empty_block_light_mask: vec![0x3FFFFFF], // All block light sections empty
            sky_light_arrays,
            block_light_arrays: Vec::new(),
        }
    }

    /// Encodes the light data to the encoder
    pub fn encode<E: PacketEncode>(&self, encoder: &mut E) -> io::Result<()> {
        encoder.encode_bitset(&self.sky_light_mask)?;
        encoder.encode_bitset(&self.block_light_mask)?;
        encoder.encode_bitset(&self.empty_sky_light_mask)?;
        encoder.encode_bitset(&self.empty_block_light_mask)?;

        // Sky light arrays
        let sky_len = i32::try_from(self.sky_light_arrays.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "too many sky light arrays"))?;
        encoder.encode_vari32(sky_len)?;
        for arr in &self.sky_light_arrays {
            let arr_len = i32::try_from(arr.len())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "sky light array too large"))?;
            encoder.encode_vari32(arr_len)?;
            encoder.write_all(arr)?;
        }

        // Block light arrays
        let block_len = i32::try_from(self.block_light_arrays.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "too many block light arrays"))?;
        encoder.encode_vari32(block_len)?;
        for arr in &self.block_light_arrays {
            let arr_len = i32::try_from(arr.len())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "block light array too large"))?;
            encoder.encode_vari32(arr_len)?;
            encoder.write_all(arr)?;
        }

        Ok(())
    }
}

// ============================================================================
// Chunk
// ============================================================================

/// Represents a complete chunk with all its data.
/// Can be converted to a `ChunkDataAndLight` packet for sending to clients.
#[derive(Debug, Clone)]
pub struct Chunk {
    /// Chunk X coordinate (in chunk coordinates, not blocks)
    pub x: i32,
    /// Chunk Z coordinate (in chunk coordinates, not blocks)
    pub z: i32,
    /// Heightmap data
    pub heightmaps: ChunkHeightmaps,
    /// Chunk sections (24 sections for a standard world, Y from -64 to 319)
    pub sections: Vec<ChunkSection>,
    /// Light data
    pub light: LightData,
}

impl Chunk {
    /// Creates an empty chunk at the given coordinates
    pub fn empty(x: i32, z: i32) -> Self {
        Self {
            x,
            z,
            heightmaps: ChunkHeightmaps::empty(),
            sections: (0..24).map(|_| ChunkSection::empty()).collect(),
            light: LightData::empty(),
        }
    }

    /// Creates a flat chunk with stone at a specific section
    pub fn flat_stone(x: i32, z: i32, stone_section: usize) -> Self {
        let mut sections: Vec<ChunkSection> = (0..24).map(|_| ChunkSection::empty()).collect();
        if stone_section < 24 {
            sections[stone_section] = ChunkSection::filled(1, 1); // Stone, Plains
        }

        Self {
            x,
            z,
            heightmaps: ChunkHeightmaps::empty(),
            sections,
            light: LightData::empty(),
        }
    }

    /// Converts this chunk to a `ChunkDataAndLight` packet
    pub fn to_packet(&self) -> ChunkDataAndLight {
        // Encode all sections to bytes
        let mut data = Vec::new();
        for section in &self.sections {
            data.extend(section.encode_to_bytes());
        }

        ChunkDataAndLight {
            chunk_x: self.x,
            chunk_z: self.z,
            heightmaps: self.heightmaps.to_nbt(),
            data,
            block_entities: Vec::new(),
            sky_light_mask: self.light.sky_light_mask.clone(),
            block_light_mask: self.light.block_light_mask.clone(),
            empty_sky_light_mask: self.light.empty_sky_light_mask.clone(),
            empty_block_light_mask: self.light.empty_block_light_mask.clone(),
            sky_light_arrays: self.light.sky_light_arrays.clone(),
            block_light_arrays: self.light.block_light_arrays.clone(),
        }
    }
}

// ============================================================================
// ChunkDataAndLight Packet
// ============================================================================

/// The Chunk Data and Update Light packet (0x28)
/// Sent to clients to provide chunk data and light information.
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

        let data_len = i32::try_from(self.data.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "data length too large"))?;
        encoder.encode_vari32(data_len)?;
        encoder.write_all(&self.data)?;

        encoder.encode_vari32(0)?; // Block entities count (0)

        // Light data
        let light = LightData {
            sky_light_mask: self.sky_light_mask.clone(),
            block_light_mask: self.block_light_mask.clone(),
            empty_sky_light_mask: self.empty_sky_light_mask.clone(),
            empty_block_light_mask: self.empty_block_light_mask.clone(),
            sky_light_arrays: self.sky_light_arrays.clone(),
            block_light_arrays: self.block_light_arrays.clone(),
        };
        light.encode(encoder)?;

        Ok(())
    }

    fn decode<D: PacketDecode>(_: &mut D) -> io::Result<Self> {
        Err(io::Error::new(io::ErrorKind::Other, "decode not implemented"))
    }
}

// ============================================================================
// SetCenterChunk Packet
// ============================================================================

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

// ============================================================================
// Helper Functions
// ============================================================================

fn write_varint(vec: &mut Vec<u8>, value: i32) {
    let mut value = value as u32;
    loop {
        if (value & !0x7F) == 0 {
            vec.push(value as u8);
            return;
        }
        vec.push(((value & 0x7F) | 0x80) as u8);
        value >>= 7;
    }
}
