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
    pub const DESERT: i32 = 0;
    pub const FOREST: i32 = 0;
    pub const SNOWY_PLAINS: i32 = 0;
    pub const BEACH: i32 = 0;
    pub const OCEAN: i32 = 0;
}

// ============================================================================
// SetCenterChunk Packet (0x58)
// ============================================================================

#[derive(Debug, Clone, Encode, Decode)]
pub struct SetCenterChunk {
    #[codec(varint32)]
    pub chunk_x: i32,
    #[codec(varint32)]
    pub chunk_z: i32,
}

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
                    motion_blocking[long_idx + 1] |=
                        ((value & 0x1FF) >> (64 - bit_offset)) as i64;
                }
            }
        }
        Self { motion_blocking }
    }

    /// Converts to NBT format for packet encoding
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
    /// Indirect palette with variable bits per entry
    Indirect {
        bits_per_entry: u8,
        palette: Vec<i32>,
        data: Vec<u64>,
    },
}

impl ChunkSection {
    /// Creates an empty section (all air, default biome)
    pub fn empty() -> Self {
        Self {
            block_count: 0,
            block_state: PaletteData::SingleValue(0), // Air
            biome: PaletteData::SingleValue(0), // First registered biome
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

    /// Creates a section with a single layer of blocks at a specific Y level within the section.
    pub fn with_layer(y_level: u8, block_state_id: i32, biome_id: i32) -> Self {
        assert!(y_level < 16, "Y level must be 0-15");

        let bits_per_entry: u8 = 4;
        let entries_per_long = 64 / bits_per_entry as usize;
        let total_longs = 4096 / entries_per_long;

        let palette = vec![0, block_state_id];

        let mut data = vec![0u64; total_longs];
        let layer_start = y_level as usize * 256;
        let layer_end = layer_start + 256;

        for block_idx in layer_start..layer_end {
            let long_idx = block_idx / entries_per_long;
            let entry_idx = block_idx % entries_per_long;
            let bit_offset = entry_idx * bits_per_entry as usize;
            data[long_idx] |= 1u64 << bit_offset;
        }

        Self {
            block_count: 256,
            block_state: PaletteData::Indirect {
                bits_per_entry,
                palette,
                data,
            },
            biome: PaletteData::SingleValue(biome_id),
        }
    }

    /// Creates a section with a floor (bottom layer, y=0) of the specified block
    pub fn with_floor(block_state_id: i32, biome_id: i32) -> Self {
        Self::with_layer(0, block_state_id, biome_id)
    }

    /// Creates a section from a 3D block array using a custom height function.
    pub fn from_heightmap<F>(height_fn: F, block_state_id: i32, biome_id: i32) -> Self
    where
        F: Fn(u8, u8) -> u8,
    {
        let mut block_data = [0i32; 4096];
        let mut block_count = 0i16;

        for x in 0..16u8 {
            for z in 0..16u8 {
                let height = height_fn(x, z).min(16);
                for y in 0..height {
                    let idx = (y as usize * 256) + (z as usize * 16) + x as usize;
                    block_data[idx] = block_state_id;
                    block_count += 1;
                }
            }
        }

        Self::from_block_array(&block_data, biome_id, block_count)
    }

    /// Creates a section from a raw block array (4096 elements, indexed as y*256 + z*16 + x)
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
            sections[stone_section] = ChunkSection::filled(1, 0);
        }

        Self {
            x,
            z,
            heightmaps: ChunkHeightmaps::empty(),
            sections,
            light: LightData::empty(),
        }
    }

    /// Creates a superflat chunk with customizable layers.
    pub fn superflat(x: i32, z: i32, layers: &[(i32, i32)]) -> Self {
        ChunkBuilder::new(x, z)
            .fill_layers(layers)
            .build()
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

/// A builder for creating chunks with complex terrain patterns.
pub struct ChunkBuilder {
    x: i32,
    z: i32,
    blocks: Vec<Vec<Vec<i32>>>,
    biome_id: i32,
}

impl ChunkBuilder {
    /// Creates a new chunk builder at the given coordinates, filled with air.
    pub fn new(x: i32, z: i32) -> Self {
        Self {
            x,
            z,
            blocks: vec![vec![vec![blocks::AIR; 16]; 16]; 384],
            biome_id: biomes::PLAINS,
        }
    }

    /// Sets the biome for the entire chunk.
    pub fn biome(mut self, biome_id: i32) -> Self {
        self.biome_id = biome_id;
        self
    }

    #[inline]
    fn y_to_index(world_y: i32) -> usize {
        (world_y + 64).clamp(0, 383) as usize
    }

    /// Sets a single block at world coordinates (relative to chunk).
    pub fn set_block(&mut self, x: u8, world_y: i32, z: u8, block: i32) {
        let y_idx = Self::y_to_index(world_y);
        if x < 16 && z < 16 {
            self.blocks[y_idx][z as usize][x as usize] = block;
        }
    }

    /// Fills all blocks below a certain Y level with the specified block.
    pub fn fill_below(mut self, world_y: i32, block: i32) -> Self {
        let y_idx = Self::y_to_index(world_y);
        for y in 0..y_idx {
            for z in 0..16 {
                for x in 0..16 {
                    self.blocks[y][z][x] = block;
                }
            }
        }
        self
    }

    /// Fills all blocks in a Y range with the specified block.
    pub fn fill_range(mut self, from_y: i32, to_y: i32, block: i32) -> Self {
        let from_idx = Self::y_to_index(from_y);
        let to_idx = Self::y_to_index(to_y);
        for y in from_idx..to_idx {
            for z in 0..16 {
                for x in 0..16 {
                    self.blocks[y][z][x] = block;
                }
            }
        }
        self
    }

    /// Fills blocks using layered materials from bottom to top.
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

    /// Applies a height function to create terrain.
    pub fn with_heightmap<F>(mut self, height_fn: F, block: i32) -> Self
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

                for y in 0..surface_idx {
                    self.blocks[y][local_z][local_x] = block;
                }
            }
        }
        self
    }

    /// Applies a height function with multiple material layers.
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

    /// Creates sine wave terrain.
    pub fn sine_terrain(self, base_y: i32, amplitude: f64, frequency: f64, block: i32) -> Self {
        self.with_heightmap(
            move |x, z| {
                let wave = (x as f64 * frequency).sin() + (z as f64 * frequency).sin();
                base_y + (wave * amplitude) as i32
            },
            block,
        )
    }

    /// Creates a circular hill/crater at a position.
    pub fn add_hill(mut self, center_x: i32, center_z: i32, radius: i32, height: i32, block: i32) -> Self {
        let base_x = self.x * 16;
        let base_z = self.z * 16;

        for local_z in 0..16 {
            for local_x in 0..16 {
                let world_x = base_x + local_x as i32;
                let world_z = base_z + local_z as i32;

                let dx = world_x - center_x;
                let dz = world_z - center_z;
                let dist_sq = dx * dx + dz * dz;
                let radius_sq = radius * radius;

                if dist_sq < radius_sq {
                    let dist = (dist_sq as f64).sqrt();
                    let factor = ((1.0 - dist / radius as f64) * std::f64::consts::PI / 2.0).cos();
                    let hill_height = (height as f64 * (1.0 - factor * factor)) as i32;

                    let mut surface_y = 0;
                    for y in (0..384).rev() {
                        if self.blocks[y][local_z][local_x] != blocks::AIR {
                            surface_y = y;
                            break;
                        }
                    }

                    for y in surface_y..(surface_y + hill_height as usize).min(384) {
                        self.blocks[y][local_z][local_x] = block;
                    }
                }
            }
        }
        self
    }

    /// Adds a water layer at a specific Y level (fills air blocks at and below that level).
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

    /// Creates a checkerboard pattern at a specific Y level.
    pub fn checkerboard(mut self, world_y: i32, block_a: i32, block_b: i32, size: i32) -> Self {
        let y_idx = Self::y_to_index(world_y);
        let base_x = self.x * 16;
        let base_z = self.z * 16;

        for z in 0..16 {
            for x in 0..16 {
                let world_x = base_x + x as i32;
                let world_z = base_z + z as i32;
                let checker = ((world_x / size) + (world_z / size)) % 2 == 0;
                self.blocks[y_idx][z][x] = if checker { block_a } else { block_b };
            }
        }
        self
    }

    /// Creates concentric rings pattern at a specific Y level.
    pub fn concentric_rings(mut self, world_y: i32, center_x: i32, center_z: i32, ring_width: i32, colors: &[i32]) -> Self {
        if colors.is_empty() {
            return self;
        }
        let y_idx = Self::y_to_index(world_y);
        let base_x = self.x * 16;
        let base_z = self.z * 16;

        for z in 0..16 {
            for x in 0..16 {
                let world_x = base_x + x as i32;
                let world_z = base_z + z as i32;
                let dx = world_x - center_x;
                let dz = world_z - center_z;
                let dist = ((dx * dx + dz * dz) as f64).sqrt() as i32;
                let ring = (dist / ring_width) as usize % colors.len();
                self.blocks[y_idx][z][x] = colors[ring];
            }
        }
        self
    }

    /// Creates a spiral pattern at a specific Y level.
    pub fn spiral(mut self, world_y: i32, center_x: i32, center_z: i32, colors: &[i32], twist: f64) -> Self {
        if colors.is_empty() {
            return self;
        }
        let y_idx = Self::y_to_index(world_y);
        let base_x = self.x * 16;
        let base_z = self.z * 16;

        for z in 0..16 {
            for x in 0..16 {
                let world_x = base_x + x as i32;
                let world_z = base_z + z as i32;
                let dx = (world_x - center_x) as f64;
                let dz = (world_z - center_z) as f64;
                let angle = dz.atan2(dx);
                let dist = (dx * dx + dz * dz).sqrt();
                let spiral_angle = angle + dist * twist;
                let segment = ((spiral_angle / (2.0 * std::f64::consts::PI) * colors.len() as f64).rem_euclid(colors.len() as f64)) as usize;
                self.blocks[y_idx][z][x] = colors[segment];
            }
        }
        self
    }

    /// Applies a gradient from one block to another based on distance from center.
    pub fn radial_gradient(mut self, world_y: i32, center_x: i32, center_z: i32, max_radius: i32, inner_block: i32, outer_block: i32) -> Self {
        let y_idx = Self::y_to_index(world_y);
        let base_x = self.x * 16;
        let base_z = self.z * 16;

        for z in 0..16 {
            for x in 0..16 {
                let world_x = base_x + x as i32;
                let world_z = base_z + z as i32;
                let dx = world_x - center_x;
                let dz = world_z - center_z;
                let dist = ((dx * dx + dz * dz) as f64).sqrt();
                let t = (dist / max_radius as f64).clamp(0.0, 1.0);
                let threshold = ((x + z) % 3) as f64 / 3.0;
                self.blocks[y_idx][z][x] = if t < threshold { inner_block } else { outer_block };
            }
        }
        self
    }

    /// Creates a 3D noise-based cave system (simple version).
    pub fn add_caves(mut self, density: f64, min_y: i32, max_y: i32) -> Self {
        let min_idx = Self::y_to_index(min_y);
        let max_idx = Self::y_to_index(max_y);
        let base_x = self.x * 16;
        let base_z = self.z * 16;

        for y in min_idx..max_idx {
            for z in 0..16 {
                for x in 0..16 {
                    let world_x = base_x + x as i32;
                    let world_z = base_z + z as i32;
                    let world_y = y as i32 - 64;

                    let hash = simple_hash(world_x, world_y, world_z);
                    if hash < density && self.blocks[y][z][x] != blocks::AIR {
                        self.blocks[y][z][x] = blocks::AIR;
                    }
                }
            }
        }
        self
    }

    /// Builds the chunk from the block data.
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

/// Simple hash function for pseudo-random generation.
fn simple_hash(x: i32, y: i32, z: i32) -> f64 {
    let n = x.wrapping_mul(374761393)
        .wrapping_add(y.wrapping_mul(668265263))
        .wrapping_add(z.wrapping_mul(1274126177));
    let n = n ^ (n >> 13);
    let n = n.wrapping_mul(1103515245);
    (n as u32 as f64) / (u32::MAX as f64)
}
