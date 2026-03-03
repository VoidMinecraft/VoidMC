use ussr_nbt::owned::Nbt;
use void_codec::{Decode, DecodeError, Encode};

pub mod blocks {
    pub const AIR: i32 = 0;
    pub const STONE: i32 = 1;
    pub const DIRT: i32 = 10;
    pub const GRASS_BLOCK: i32 = 8; // snowy=false
    pub const WATER: i32 = 86; // level=0
}

pub mod biomes {
    pub const PLAINS: i32 = 0;
}

// ============================================================================
// Chunk Heightmaps
// ============================================================================

#[derive(Debug, Clone, Default)]
pub struct ChunkHeightmaps {
    pub motion_blocking: Vec<i64>,
}

impl ChunkHeightmaps {
    pub fn empty() -> Self {
        Self {
            motion_blocking: vec![0i64; 37],
        }
    }

    pub fn flat(surface_y: i32) -> Self {
        let mut motion_blocking = vec![0i64; 37];
        let value = (surface_y + 64) as u64;
        for i in 0..256 {
            let long_idx = (i * 9) / 64;
            let bit_offset = (i * 9) % 64;
            if long_idx < 37 {
                motion_blocking[long_idx] |= ((value & 0x1FF) << bit_offset) as i64;
                if bit_offset > 55 && long_idx + 1 < 37 {
                    motion_blocking[long_idx + 1] |=
                        ((value & 0x1FF) >> (64 - bit_offset)) as i64;
                }
            }
        }
        Self { motion_blocking }
    }

    pub fn to_nbt(&self) -> Nbt {
        Nbt {
            name: "".into(),
            compound: vec![("MOTION_BLOCKING".into(), self.motion_blocking.clone().into())]
                .into(),
        }
    }
}

// ============================================================================
// Chunk Section
// ============================================================================

#[derive(Debug, Clone)]
pub struct ChunkSection {
    pub block_count: i16,
    pub block_state: PaletteData,
    pub biome: PaletteData,
}

#[derive(Debug, Clone)]
pub enum PaletteData {
    SingleValue(i32),
    Indirect {
        bits_per_entry: u8,
        palette: Vec<i32>,
        data: Vec<u64>,
    },
}

impl ChunkSection {
    pub fn empty() -> Self {
        Self {
            block_count: 0,
            block_state: PaletteData::SingleValue(0),
            biome: PaletteData::SingleValue(0),
        }
    }

    pub fn filled(block_state_id: i32, biome_id: i32) -> Self {
        Self {
            block_count: if block_state_id == 0 { 0 } else { 4096 },
            block_state: PaletteData::SingleValue(block_state_id),
            biome: PaletteData::SingleValue(biome_id),
        }
    }

    pub fn from_block_array(block_array: &[i32; 4096], biome_id: i32, block_count: i16) -> Self {
        let mut palette: Vec<i32> = vec![blocks::AIR];
        for &block in block_array {
            if block != blocks::AIR && !palette.contains(&block) {
                palette.push(block);
            }
        }

        if palette.len() == 1 && block_count == 0 {
            return Self::empty();
        }

        if palette.len() == 2 && block_count == 4096 {
            return Self::filled(palette[1], biome_id);
        }

        let bits_per_entry = ((palette.len() as f64).log2().ceil() as u8).max(4);
        let entries_per_long = 64 / bits_per_entry as usize;
        let total_longs = (4096 + entries_per_long - 1) / entries_per_long;

        let mut data = vec![0u64; total_longs];
        let mask = (1u64 << bits_per_entry) - 1;

        for (idx, &block) in block_array.iter().enumerate() {
            let palette_idx = palette.iter().position(|&b| b == block).unwrap_or(0) as u64;
            let long_idx = idx / entries_per_long;
            let bit_offset = (idx % entries_per_long) * bits_per_entry as usize;
            data[long_idx] |= (palette_idx & mask) << bit_offset;
        }

        Self {
            block_count,
            block_state: PaletteData::Indirect {
                bits_per_entry,
                palette,
                data,
            },
            biome: PaletteData::SingleValue(biome_id),
        }
    }

    pub fn encode_to_bytes(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&self.block_count.to_be_bytes());

        match &self.block_state {
            PaletteData::SingleValue(id) => {
                data.push(0);
                write_varint(&mut data, *id);
                write_varint(&mut data, 0);
            }
            PaletteData::Indirect {
                bits_per_entry,
                palette,
                data: block_data,
            } => {
                data.push(*bits_per_entry);
                write_varint(&mut data, palette.len() as i32);
                for &id in palette {
                    write_varint(&mut data, id);
                }
                write_varint(&mut data, block_data.len() as i32);
                for &long in block_data {
                    data.extend_from_slice(&long.to_be_bytes());
                }
            }
        }

        match &self.biome {
            PaletteData::SingleValue(id) => {
                data.push(0);
                write_varint(&mut data, *id);
                write_varint(&mut data, 0);
            }
            PaletteData::Indirect {
                bits_per_entry,
                palette,
                data: biome_data,
            } => {
                data.push(*bits_per_entry);
                write_varint(&mut data, palette.len() as i32);
                for &id in palette {
                    write_varint(&mut data, id);
                }
                write_varint(&mut data, biome_data.len() as i32);
                for &long in biome_data {
                    data.extend_from_slice(&long.to_be_bytes());
                }
            }
        }

        data
    }
}

// ============================================================================
// Light Data
// ============================================================================

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

#[derive(Debug, Clone)]
pub struct Chunk {
    pub x: i32,
    pub z: i32,
    pub heightmaps: ChunkHeightmaps,
    pub sections: Vec<ChunkSection>,
    pub light: LightData,
}

impl Chunk {
    pub fn empty(x: i32, z: i32) -> Self {
        Self {
            x,
            z,
            heightmaps: ChunkHeightmaps::empty(),
            sections: (0..24).map(|_| ChunkSection::empty()).collect(),
            light: LightData::empty(),
        }
    }

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
// ChunkDataAndLight Packet
// ============================================================================

#[derive(Debug, Clone)]
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
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.chunk_x.to_be_bytes());
        buf.extend_from_slice(&self.chunk_z.to_be_bytes());

        self.heightmaps.encode(buf);

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
    }
}

impl Decode for ChunkDataAndLight {
    fn decode(_buf: &mut &[u8]) -> Result<Self, DecodeError> {
        Err(DecodeError::InvalidLength)
    }
}

// ============================================================================
// ChunkBuilder
// ============================================================================

pub struct ChunkBuilder {
    x: i32,
    z: i32,
    blocks: Vec<Vec<Vec<i32>>>,
    biome_id: i32,
}

impl ChunkBuilder {
    pub fn new(x: i32, z: i32) -> Self {
        Self {
            x,
            z,
            blocks: vec![vec![vec![blocks::AIR; 16]; 16]; 384],
            biome_id: biomes::PLAINS,
        }
    }

    #[inline]
    fn y_to_index(world_y: i32) -> usize {
        (world_y + 64).clamp(0, 383) as usize
    }

    pub fn set_block(&mut self, x: u8, world_y: i32, z: u8, block: i32) {
        let y_idx = Self::y_to_index(world_y);
        if x < 16 && z < 16 {
            self.blocks[y_idx][z as usize][x as usize] = block;
        }
    }

    pub fn fill_layers(mut self, layers: &[(i32, i32)]) -> Self {
        let mut current_y = 0usize;
        for &(thickness, block) in layers {
            let end_y = (current_y + thickness as usize).min(384);
            for y in current_y..end_y {
                for z in 0..16 {
                    for x in 0..16 {
                        self.blocks[y][z][x] = block;
                    }
                }
            }
            current_y = end_y;
        }
        self
    }

    pub fn with_heightmap_layered<F>(mut self, height_fn: F, layers: &[(i32, i32)]) -> Self
    where
        F: Fn(i32, i32) -> i32,
    {
        let base_x = self.x * 16;
        let base_z = self.z * 16;

        for local_z in 0..16 {
            for local_x in 0..16 {
                let world_x = base_x + local_x as i32;
                let world_z = base_z + local_z as i32;
                let surface_y = height_fn(world_x, world_z);
                let surface_idx = Self::y_to_index(surface_y);

                let mut depth = 0i32;
                for y in (0..surface_idx).rev() {
                    let block = layers
                        .iter()
                        .find(|(d, _)| depth < *d)
                        .map(|(_, b)| *b)
                        .unwrap_or(blocks::STONE);

                    self.blocks[y][local_z][local_x] = block;
                    depth += 1;
                }
            }
        }
        self
    }

    pub fn add_water(mut self, water_level: i32) -> Self {
        let water_idx = Self::y_to_index(water_level);
        for y in 0..=water_idx {
            for z in 0..16 {
                for x in 0..16 {
                    if self.blocks[y][z][x] == blocks::AIR {
                        self.blocks[y][z][x] = blocks::WATER;
                    }
                }
            }
        }
        self
    }

    pub fn build(self) -> Chunk {
        let mut sections = Vec::with_capacity(24);

        for section_idx in 0..24 {
            let base_y = section_idx * 16;
            let mut block_array = [blocks::AIR; 4096];
            let mut block_count = 0i16;

            for y in 0..16 {
                for z in 0..16 {
                    for x in 0..16 {
                        let world_y_idx = base_y + y;
                        let block = self.blocks[world_y_idx][z][x];
                        if block != blocks::AIR {
                            block_count += 1;
                        }
                        block_array[y * 256 + z * 16 + x] = block;
                    }
                }
            }

            sections.push(ChunkSection::from_block_array(
                &block_array,
                self.biome_id,
                block_count,
            ));
        }

        Chunk {
            x: self.x,
            z: self.z,
            heightmaps: ChunkHeightmaps::empty(),
            sections,
            light: LightData::full_sky_light(),
        }
    }
}

