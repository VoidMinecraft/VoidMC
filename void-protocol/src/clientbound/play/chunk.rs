use ussr_nbt::owned::Nbt;
use void_codec::{Decode, Encode};

// ============================================================================
// Chunk Heightmaps
// ============================================================================

/// Represents the heightmap data for a chunk.
#[derive(Debug, Clone, Default)]
pub struct ChunkHeightmaps {
    /// MOTION_BLOCKING heightmap: 256 values packed into 37 longs (9 bits per value)
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
        let value = (surface_y + 64) as u64;
        for i in 0..256 {
            let long_idx = (i * 9) / 64;
            let bit_offset = (i * 9) % 64;
            if long_idx < 37 {
                motion_blocking[long_idx] |= ((value & 0x1FF) << bit_offset) as i64;
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
    /// Block state palette entry
    pub block_state: PaletteData,
    /// Biome palette entry
    pub biome: PaletteData,
}

/// Palette data for blocks or biomes
#[derive(Debug, Clone)]
pub enum PaletteData {
    /// Single value palette (bits per entry = 0)
    SingleValue(i32),
}

impl ChunkSection {
    /// Creates an empty section (all air, plains biome)
    pub fn empty() -> Self {
        Self {
            block_count: 0,
            block_state: PaletteData::SingleValue(0),
            biome: PaletteData::SingleValue(1),
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
        data.extend_from_slice(&self.block_count.to_be_bytes());

        match &self.block_state {
            PaletteData::SingleValue(id) => {
                data.push(0);
                write_varint(&mut data, *id);
                write_varint(&mut data, 0);
            }
        }

        match &self.biome {
            PaletteData::SingleValue(id) => {
                data.push(0);
                write_varint(&mut data, *id);
                write_varint(&mut data, 0);
            }
        }

        data
    }
}

// ============================================================================
// Light Data
// ============================================================================

/// Represents light data for a chunk.
#[derive(Debug, Clone, Default)]
pub struct LightData {
    pub sky_light_mask: Vec<u64>,
    pub block_light_mask: Vec<u64>,
    pub empty_sky_light_mask: Vec<u64>,
    pub empty_block_light_mask: Vec<u64>,
    pub sky_light_arrays: Vec<Vec<u8>>,
    pub block_light_arrays: Vec<Vec<u8>>,
}

impl LightData {
    /// Creates empty light data (all sections marked as empty)
    pub fn empty() -> Self {
        Self {
            sky_light_mask: vec![0],
            block_light_mask: vec![0],
            empty_sky_light_mask: vec![0x3FFFFFF],
            empty_block_light_mask: vec![0x3FFFFFF],
            sky_light_arrays: Vec::new(),
            block_light_arrays: Vec::new(),
        }
    }

    /// Creates light data with full sky light for all sections
    pub fn full_sky_light() -> Self {
        let num_sections = 26;
        let mut sky_light_arrays = Vec::with_capacity(num_sections);
        for _ in 0..num_sections {
            sky_light_arrays.push(vec![0xFF; 2048]);
        }

        Self {
            sky_light_mask: vec![0x3FFFFFF],
            block_light_mask: vec![0],
            empty_sky_light_mask: vec![0],
            empty_block_light_mask: vec![0x3FFFFFF],
            sky_light_arrays,
            block_light_arrays: Vec::new(),
        }
    }
}

// ============================================================================
// Chunk
// ============================================================================

/// Represents a complete chunk with all its data.
#[derive(Debug, Clone)]
pub struct Chunk {
    pub x: i32,
    pub z: i32,
    pub heightmaps: ChunkHeightmaps,
    pub sections: Vec<ChunkSection>,
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
            sections[stone_section] = ChunkSection::filled(1, 1);
        }

        Self {
            x,
            z,
            heightmaps: ChunkHeightmaps::empty(),
            sections,
            light: LightData::empty(),
        }
    }

    /// Converts the chunk to a ChunkDataAndLight packet
    pub fn to_packet(&self) -> ChunkDataAndLight {
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
// SetCenterChunk Packet
// ============================================================================

#[derive(Debug, Encode, Decode)]
pub struct SetCenterChunk {
    #[codec(varint32)]
    pub chunk_x: i32,
    #[codec(varint32)]
    pub chunk_z: i32,
}

// ============================================================================
// ChunkDataAndLight Packet
// ============================================================================

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

        ussr_nbt::encode(&self.heightmaps, buf);

        write_varint(buf, self.data.len() as i32);
        buf.extend_from_slice(&self.data);

        write_varint(buf, 0); // Block entities count

        write_bitset(buf, &self.sky_light_mask);
        write_bitset(buf, &self.block_light_mask);
        write_bitset(buf, &self.empty_sky_light_mask);
        write_bitset(buf, &self.empty_block_light_mask);

        write_varint(buf, self.sky_light_arrays.len() as i32);
        for arr in &self.sky_light_arrays {
            write_varint(buf, arr.len() as i32);
            buf.extend_from_slice(arr);
        }

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
