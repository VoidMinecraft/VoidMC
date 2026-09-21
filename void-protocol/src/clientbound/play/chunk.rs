use voidmc_codec::{Decode, DecodeError, Encode};

pub mod blocks {
    pub const AIR: i32 = 0;
    pub const STONE: i32 = 1;
    pub const DIRT: i32 = 10;
    pub const GRASS_BLOCK: i32 = 9; // snowy=false (id 8 is snowy=true)
    pub const WATER: i32 = 86; // level=0
}

// Biome ids are the send order of the synced `minecraft:worldgen/biome`
// registry, so they can only be looked up, never hardcoded.
pub mod biomes {
    use std::sync::OnceLock;

    const REGISTRY: &str = "minecraft:worldgen/biome";

    pub fn id(name: &str) -> Option<i32> {
        voidmc_data::registry_index(voidmc_data::Version::V26_1_2, REGISTRY, name)
    }

    /// Entry count of the shipped biome registry (before any custom biome).
    pub fn vanilla_count() -> usize {
        voidmc_data::registry(voidmc_data::Version::V26_1_2, REGISTRY)
            .map(|entries| entries.len())
            .unwrap_or(0)
    }

    fn resolve(cell: &OnceLock<i32>, name: &str) -> i32 {
        *cell.get_or_init(|| {
            id(name).unwrap_or_else(|| panic!("biome {name} is not in the synced registry"))
        })
    }

    macro_rules! vanilla_biome {
        ($fn:ident, $name:literal) => {
            pub fn $fn() -> i32 {
                static ID: OnceLock<i32> = OnceLock::new();
                resolve(&ID, $name)
            }
        };
    }

    // The client rejects the Play `Login` packet if `minecraft:plains` is absent.
    vanilla_biome!(plains, "minecraft:plains");
    vanilla_biome!(desert, "minecraft:desert");
    vanilla_biome!(forest, "minecraft:forest");
    vanilla_biome!(snowy_plains, "minecraft:snowy_plains");
    vanilla_biome!(beach, "minecraft:beach");
    vanilla_biome!(ocean, "minecraft:ocean");
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

/// Heightmap type IDs as defined by `Heightmap.Types.id` in vanilla.
/// Only types with `Usage.CLIENT` are sent over the wire (1 and 4).
pub const HEIGHTMAP_TYPE_WORLD_SURFACE: i32 = 1;
pub const HEIGHTMAP_TYPE_MOTION_BLOCKING: i32 = 4;

/// Represents the heightmap data for a chunk.
///
/// Wire format (1.21.5+): `varint count` followed by, for each entry,
/// `varint type_id` and a `long_array` (varint length + i64s).
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
#[derive(Debug, Clone, PartialEq)]
pub enum PaletteData {
    /// Single value palette (bits per entry = 0)
    SingleValue(i32),
    /// Indirect palette with variable bits per entry
    Indirect {
        bits_per_entry: u8,
        palette: Vec<i32>,
        data: Vec<u64>,
    },
    /// Global ids packed at `ceil(log2(registry size))` bits, no palette
    Direct { bits_per_entry: u8, data: Vec<u64> },
}

impl PaletteData {
    pub fn bits_per_entry(&self) -> u8 {
        match self {
            PaletteData::SingleValue(_) => 0,
            PaletteData::Indirect { bits_per_entry, .. }
            | PaletteData::Direct { bits_per_entry, .. } => *bits_per_entry,
        }
    }

    fn entry(&self, index: usize) -> i32 {
        match self {
            PaletteData::SingleValue(id) => *id,
            PaletteData::Indirect {
                bits_per_entry,
                palette,
                data,
            } => palette
                .get(packed_get(data, *bits_per_entry, index) as usize)
                .copied()
                .unwrap_or(0),
            PaletteData::Direct {
                bits_per_entry,
                data,
            } => packed_get(data, *bits_per_entry, index) as i32,
        }
    }

    fn write(&self, buf: &mut Vec<u8>) {
        buf.push(self.bits_per_entry());
        match self {
            PaletteData::SingleValue(id) => write_varint(buf, *id),
            PaletteData::Indirect { palette, data, .. } => {
                write_varint(buf, palette.len() as i32);
                for &id in palette {
                    write_varint(buf, id);
                }
                for &long in data {
                    buf.extend_from_slice(&long.to_be_bytes());
                }
            }
            PaletteData::Direct { data, .. } => {
                for &long in data {
                    buf.extend_from_slice(&long.to_be_bytes());
                }
            }
        }
    }
}

pub const BIOME_CELLS_PER_SECTION: usize = 64;
const BIOME_MAX_INDIRECT_BITS: u8 = 3;

/// Bits per entry of a direct biome palette: `ceil(log2(registry size))`.
pub fn biome_direct_bits(registry_size: usize) -> u8 {
    ceil_log2(registry_size).max(1)
}

fn ceil_log2(count: usize) -> u8 {
    if count <= 1 {
        0
    } else {
        (usize::BITS - (count - 1).leading_zeros()) as u8
    }
}

fn packed_len(bits: u8, entries: usize) -> usize {
    entries.div_ceil(64 / bits as usize)
}

fn packed_get(data: &[u64], bits: u8, index: usize) -> u64 {
    let per_long = 64 / bits as usize;
    let mask = (1u64 << bits) - 1;
    let long = data.get(index / per_long).copied().unwrap_or(0);
    (long >> ((index % per_long) * bits as usize)) & mask
}

fn packed_set(data: &mut [u64], bits: u8, index: usize, value: u64) {
    let per_long = 64 / bits as usize;
    let mask = (1u64 << bits) - 1;
    let shift = (index % per_long) * bits as usize;
    let long = &mut data[index / per_long];
    *long = (*long & !(mask << shift)) | ((value & mask) << shift);
}

/// Biome container for 64 cells: single value, indirect at 1..=3 bits, or
/// direct at `biome_direct_bits(registry_size)` — the only widths the client accepts.
pub fn biome_palette(cells: &[i32; BIOME_CELLS_PER_SECTION], registry_size: usize) -> PaletteData {
    let mut palette: Vec<i32> = Vec::new();
    for &id in cells {
        if !palette.contains(&id) {
            palette.push(id);
        }
    }
    if palette.len() == 1 {
        return PaletteData::SingleValue(palette[0]);
    }
    let bits = ceil_log2(palette.len()).max(1);
    if bits <= BIOME_MAX_INDIRECT_BITS {
        let mut data = vec![0u64; packed_len(bits, BIOME_CELLS_PER_SECTION)];
        for (index, id) in cells.iter().enumerate() {
            let value = palette.iter().position(|p| p == id).unwrap() as u64;
            packed_set(&mut data, bits, index, value);
        }
        PaletteData::Indirect {
            bits_per_entry: bits,
            palette,
            data,
        }
    } else {
        let bits = biome_direct_bits(registry_size);
        let mut data = vec![0u64; packed_len(bits, BIOME_CELLS_PER_SECTION)];
        for (index, &id) in cells.iter().enumerate() {
            packed_set(&mut data, bits, index, id as u64);
        }
        PaletteData::Direct {
            bits_per_entry: bits,
            data,
        }
    }
}

fn biome_cell_index(x: u8, y: u8, z: u8) -> usize {
    debug_assert!(x < 4 && y < 4 && z < 4);
    (y as usize) * 16 + (z as usize) * 4 + (x as usize)
}

impl ChunkSection {
    /// Creates an empty section (all air, default biome)
    pub fn empty() -> Self {
        Self {
            block_count: 0,
            block_state: PaletteData::SingleValue(0), // Air
            biome: PaletteData::SingleValue(0),       // First registered biome
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
        let total_longs = 4096_usize.div_ceil(entries_per_long);

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

    /// Reads the block-state id at the given local section coordinates (0..16).
    pub fn get_block_state(&self, x: u8, y: u8, z: u8) -> i32 {
        debug_assert!(x < 16 && y < 16 && z < 16);
        let idx = (y as usize) * 256 + (z as usize) * 16 + (x as usize);
        self.block_state.entry(idx)
    }

    /// Writes a block-state id at the given local section coordinates and returns
    /// the previous value.
    ///
    /// Promotes a `SingleValue` palette to `Indirect` on the first heterogeneous
    /// write, and grows `bits_per_entry` as the palette outgrows the current
    /// width. The block count is updated to track the number of non-air blocks.
    pub fn set_block_state(&mut self, x: u8, y: u8, z: u8, new_id: i32) -> i32 {
        debug_assert!(x < 16 && y < 16 && z < 16);
        let idx = (y as usize) * 256 + (z as usize) * 16 + (x as usize);

        let old_id = self.get_block_state(x, y, z);
        if old_id == new_id {
            return old_id;
        }

        match &mut self.block_state {
            PaletteData::SingleValue(cur) => {
                let cur_id = *cur;
                let bits_per_entry: u8 = 4;
                let entries_per_long = 64 / bits_per_entry as usize;
                let total_longs = 4096_usize.div_ceil(entries_per_long);
                let palette = vec![cur_id, new_id];

                // palette[0] = cur_id, so a zero-filled data buffer already maps
                // every cell to the previous block — only the target needs to flip
                // to palette index 1 (new_id).
                let mut data = vec![0u64; total_longs];

                let long_idx = idx / entries_per_long;
                let bit_offset = (idx % entries_per_long) * bits_per_entry as usize;
                let mask = (1u64 << bits_per_entry) - 1;
                data[long_idx] &= !(mask << bit_offset);
                data[long_idx] |= (1u64 & mask) << bit_offset;

                self.block_state = PaletteData::Indirect {
                    bits_per_entry,
                    palette,
                    data,
                };
            }
            PaletteData::Indirect {
                bits_per_entry,
                palette,
                data,
            } => {
                let palette_idx = match palette.iter().position(|&id| id == new_id) {
                    Some(i) => i,
                    None => {
                        palette.push(new_id);
                        palette.len() - 1
                    }
                };

                let needed_bits = palette_bits_needed(palette.len());
                if needed_bits > *bits_per_entry {
                    let old_bits = *bits_per_entry as usize;
                    let new_bits = needed_bits as usize;
                    let old_per_long = 64 / old_bits;
                    let new_per_long = 64 / new_bits;
                    let new_total_longs = 4096_usize.div_ceil(new_per_long);
                    let old_mask = (1u64 << old_bits) - 1;
                    let new_mask = (1u64 << new_bits) - 1;

                    let mut new_data = vec![0u64; new_total_longs];
                    for i in 0..4096 {
                        let old_long = i / old_per_long;
                        let old_off = (i % old_per_long) * old_bits;
                        let v = (data[old_long] >> old_off) & old_mask;
                        let new_long = i / new_per_long;
                        let new_off = (i % new_per_long) * new_bits;
                        new_data[new_long] |= (v & new_mask) << new_off;
                    }
                    *data = new_data;
                    *bits_per_entry = needed_bits;
                }

                let bits = *bits_per_entry as usize;
                let entries_per_long = 64 / bits;
                let long_idx = idx / entries_per_long;
                let bit_offset = (idx % entries_per_long) * bits;
                let mask = (1u64 << bits) - 1;
                data[long_idx] &= !(mask << bit_offset);
                data[long_idx] |= (palette_idx as u64 & mask) << bit_offset;
            }
            PaletteData::Direct {
                bits_per_entry,
                data,
            } => packed_set(data, *bits_per_entry, idx, new_id as u64),
        }

        if old_id == blocks::AIR && new_id != blocks::AIR {
            self.block_count = self.block_count.saturating_add(1);
        } else if old_id != blocks::AIR && new_id == blocks::AIR {
            self.block_count = self.block_count.saturating_sub(1);
        }

        old_id
    }

    /// Biome of a 4×4×4 cell, addressed by cell coordinates (0..4) inside the section.
    pub fn get_biome(&self, cell_x: u8, cell_y: u8, cell_z: u8) -> i32 {
        self.biome.entry(biome_cell_index(cell_x, cell_y, cell_z))
    }

    /// Sets a biome cell and returns the previous id. `registry_size` is the
    /// number of entries in the synced biome registry, which fixes the width of
    /// a direct palette.
    pub fn set_biome(
        &mut self,
        cell_x: u8,
        cell_y: u8,
        cell_z: u8,
        biome_id: i32,
        registry_size: usize,
    ) -> i32 {
        let index = biome_cell_index(cell_x, cell_y, cell_z);
        let old = self.biome.entry(index);
        if old == biome_id {
            return old;
        }
        let mut cells = self.biome_cells();
        cells[index] = biome_id;
        self.biome = biome_palette(&cells, registry_size);
        old
    }

    pub fn biome_cells(&self) -> [i32; BIOME_CELLS_PER_SECTION] {
        std::array::from_fn(|index| self.biome.entry(index))
    }

    /// The biome container alone, as carried by `ChunksBiomes`.
    pub fn encode_biomes_to_bytes(&self) -> Vec<u8> {
        let mut data = Vec::new();
        self.biome.write(&mut data);
        data
    }

    /// Encodes this section to bytes.
    ///
    /// Wire format (1.21.5+, `PalettedContainer.read`):
    ///   - SingleValue: `[bits=0, varint(value)]` (no data array — `ZeroBitStorage`).
    ///   - Indirect:    `[bits, varint(palette_len), varint × palette_len, i64 × N]`
    ///     where `N` is implicit from `bits` and the entry count, **no length prefix**.
    pub fn encode_to_bytes(&self) -> Vec<u8> {
        let mut data = Vec::new();
        // 1.21.5+: section header is two shorts — non-empty block count
        // (`block_count`) and fluid count. We don't track fluids, so it stays 0.
        data.extend_from_slice(&self.block_count.to_be_bytes());
        data.extend_from_slice(&0_i16.to_be_bytes());
        self.block_state.write(&mut data);
        self.biome.write(&mut data);
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
        ChunkBuilder::new(x, z).fill_layers(layers).build()
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
            heightmaps: self.heightmaps.clone(),
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
    pub heightmaps: ChunkHeightmaps,
    pub data: Vec<u8>,
    pub block_entities: Vec<u8>,
    pub sky_light_mask: Vec<u64>,
    pub block_light_mask: Vec<u64>,
    pub empty_sky_light_mask: Vec<u64>,
    pub empty_block_light_mask: Vec<u64>,
    pub sky_light_arrays: Vec<Vec<u8>>,
    pub block_light_arrays: Vec<Vec<u8>>,
}

/// Returns the indirect-palette `bits_per_entry` required to index `len` entries.
/// Matches vanilla: 4 bits minimum, then ceil(log2(len)).
fn palette_bits_needed(len: usize) -> u8 {
    if len <= 16 {
        4
    } else {
        let mut bits = 5u8;
        while (1usize << bits) < len && bits < 8 {
            bits += 1;
        }
        bits
    }
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

        // Heightmaps (1.21.5+): varint count + (varint type_id + long_array) × N.
        // We only ship MOTION_BLOCKING; the client tolerates a partial map.
        write_varint(buf, 1);
        write_varint(buf, HEIGHTMAP_TYPE_MOTION_BLOCKING);
        write_varint(buf, self.heightmaps.motion_blocking.len() as i32);
        for &long in &self.heightmaps.motion_blocking {
            buf.extend_from_slice(&long.to_be_bytes());
        }

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
    fn decode_with(_decoder: &mut voidmc_codec::Decoder<'_>) -> Result<Self, DecodeError> {
        Err(DecodeError::InvalidLength)
    }
}

// ============================================================================
// ChunksBiomes Packet (0x0D)
// ============================================================================

/// Biome containers of one loaded chunk, bottom section first. The chunk
/// position travels as one long, so `z` precedes `x` on the wire.
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkBiomeData {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub data: Vec<u8>,
}

impl ChunkBiomeData {
    pub fn from_sections(chunk_x: i32, chunk_z: i32, sections: &[ChunkSection]) -> Self {
        Self {
            chunk_x,
            chunk_z,
            data: sections
                .iter()
                .flat_map(ChunkSection::encode_biomes_to_bytes)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChunksBiomes {
    pub chunks: Vec<ChunkBiomeData>,
}

impl Encode for ChunksBiomes {
    fn encode(&self, buf: &mut Vec<u8>) {
        write_varint(buf, self.chunks.len() as i32);
        for chunk in &self.chunks {
            buf.extend_from_slice(&chunk.chunk_z.to_be_bytes());
            buf.extend_from_slice(&chunk.chunk_x.to_be_bytes());
            write_varint(buf, chunk.data.len() as i32);
            buf.extend_from_slice(&chunk.data);
        }
    }
}

impl Decode for ChunksBiomes {
    fn decode_with(decoder: &mut voidmc_codec::Decoder<'_>) -> Result<Self, DecodeError> {
        let count = decoder.decode::<voidmc_codec::VarI32>()?.0;
        let mut chunks = Vec::new();
        for _ in 0..count {
            let chunk_z = decoder.decode::<i32>()?;
            let chunk_x = decoder.decode::<i32>()?;
            let data = decoder.decode::<Vec<u8>>()?;
            chunks.push(ChunkBiomeData {
                chunk_x,
                chunk_z,
                data,
            });
        }
        Ok(ChunksBiomes { chunks })
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
    biomes: Vec<i32>,
    biome_registry_size: usize,
}

impl ChunkBuilder {
    /// Creates a new chunk builder at the given coordinates, filled with air.
    pub fn new(x: i32, z: i32) -> Self {
        Self {
            x,
            z,
            blocks: vec![vec![vec![blocks::AIR; 16]; 16]; 384],
            biomes: vec![biomes::plains(); 24 * BIOME_CELLS_PER_SECTION],
            biome_registry_size: biomes::vanilla_count(),
        }
    }

    /// Sets the biome for the entire chunk.
    pub fn biome(mut self, biome_id: i32) -> Self {
        self.biomes.fill(biome_id);
        self
    }

    /// Number of entries in the synced biome registry; only needed when custom
    /// biomes were added, since it fixes the width of direct biome palettes.
    pub fn biome_registry_size(mut self, size: usize) -> Self {
        self.biome_registry_size = size;
        self
    }

    /// Sets every 4×4×4 biome cell from the world block coordinates of its
    /// lowest corner.
    pub fn biomes_from(mut self, mut biome_at: impl FnMut(i32, i32, i32) -> i32) -> Self {
        for (index, cell) in self.biomes.iter_mut().enumerate() {
            let section = (index / BIOME_CELLS_PER_SECTION) as i32;
            let local = index % BIOME_CELLS_PER_SECTION;
            let (cx, cy, cz) = (
                (local % 4) as i32,
                (local / 16) as i32,
                ((local / 4) % 4) as i32,
            );
            *cell = biome_at(
                self.x * 16 + cx * 4,
                -64 + section * 16 + cy * 4,
                self.z * 16 + cz * 4,
            );
        }
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

                for (depth, y) in (0..surface_idx).rev().enumerate() {
                    let depth = depth as i32;
                    let block = layers
                        .iter()
                        .find(|(d, _)| depth < *d)
                        .map(|(_, b)| *b)
                        .unwrap_or(blocks::STONE);

                    self.blocks[y][local_z][local_x] = block;
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
    pub fn add_hill(
        mut self,
        center_x: i32,
        center_z: i32,
        radius: i32,
        height: i32,
        block: i32,
    ) -> Self {
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
    pub fn concentric_rings(
        mut self,
        world_y: i32,
        center_x: i32,
        center_z: i32,
        ring_width: i32,
        colors: &[i32],
    ) -> Self {
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
    pub fn spiral(
        mut self,
        world_y: i32,
        center_x: i32,
        center_z: i32,
        colors: &[i32],
        twist: f64,
    ) -> Self {
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
                let segment = ((spiral_angle / (2.0 * std::f64::consts::PI) * colors.len() as f64)
                    .rem_euclid(colors.len() as f64)) as usize;
                self.blocks[y_idx][z][x] = colors[segment];
            }
        }
        self
    }

    /// Applies a gradient from one block to another based on distance from center.
    pub fn radial_gradient(
        mut self,
        world_y: i32,
        center_x: i32,
        center_z: i32,
        max_radius: i32,
        inner_block: i32,
        outer_block: i32,
    ) -> Self {
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
                self.blocks[y_idx][z][x] = if t < threshold {
                    inner_block
                } else {
                    outer_block
                };
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

            let mut section = ChunkSection::from_block_array(&block_array, 0, block_count);
            let cells: [i32; BIOME_CELLS_PER_SECTION] = self.biomes[section_idx
                * BIOME_CELLS_PER_SECTION
                ..(section_idx + 1) * BIOME_CELLS_PER_SECTION]
                .try_into()
                .unwrap();
            section.biome = biome_palette(&cells, self.biome_registry_size);
            sections.push(section);
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
    let n = x
        .wrapping_mul(374761393)
        .wrapping_add(y.wrapping_mul(668265263))
        .wrapping_add(z.wrapping_mul(1274126177));
    let n = n ^ (n >> 13);
    let n = n.wrapping_mul(1103515245);
    (n as u32 as f64) / (u32::MAX as f64)
}

#[cfg(test)]
mod biome_tests {
    use super::*;
    use crate::clientbound::PlayPacket;

    fn cells(f: impl Fn(usize) -> i32) -> [i32; BIOME_CELLS_PER_SECTION] {
        std::array::from_fn(f)
    }

    #[test]
    fn direct_bits_follow_registry_size() {
        assert_eq!(biome_direct_bits(1), 1);
        assert_eq!(biome_direct_bits(2), 1);
        assert_eq!(biome_direct_bits(3), 2);
        assert_eq!(biome_direct_bits(64), 6);
        assert_eq!(biome_direct_bits(65), 7);
        assert_eq!(biome_direct_bits(128), 7);
        assert_eq!(biome_direct_bits(129), 8);
        assert_eq!(biome_direct_bits(biomes::vanilla_count()), 7);
    }

    #[test]
    fn palette_widths_are_only_zero_one_to_three_or_direct() {
        for registry_size in [65usize, 128, 129, 300] {
            let direct = biome_direct_bits(registry_size);
            for distinct in 1..=12usize {
                let palette = biome_palette(&cells(|i| (i % distinct) as i32), registry_size);
                let bits = palette.bits_per_entry();
                match distinct {
                    1 => assert_eq!(bits, 0),
                    2 => assert_eq!(bits, 1),
                    3..=4 => assert_eq!(bits, 2),
                    5..=8 => assert_eq!(bits, 3),
                    _ => {
                        assert_eq!(bits, direct);
                        assert!(matches!(palette, PaletteData::Direct { .. }));
                    }
                }
            }
        }
    }

    #[test]
    fn cells_survive_every_palette_shape() {
        for distinct in [1usize, 2, 5, 9, 64] {
            let expected = cells(|i| ((i * 7) % distinct) as i32 + 10);
            let mut section = ChunkSection::empty();
            section.biome = biome_palette(&expected, 65);
            assert_eq!(section.biome_cells(), expected);
            assert_eq!(section.get_biome(3, 2, 1), expected[2 * 16 + 4 + 3]);
        }
    }

    #[test]
    fn set_biome_promotes_and_demotes_palettes() {
        let mut section = ChunkSection::filled(1, 3);
        assert_eq!(section.set_biome(0, 0, 0, 3, 65), 3);
        assert_eq!(section.biome, PaletteData::SingleValue(3));

        assert_eq!(section.set_biome(1, 2, 3, 40, 65), 3);
        assert_eq!(section.biome.bits_per_entry(), 1);
        assert_eq!(section.get_biome(1, 2, 3), 40);
        assert_eq!(section.get_biome(0, 0, 0), 3);

        for (i, id) in (50..58).enumerate() {
            section.set_biome(i as u8 % 4, 3, 2 + i as u8 / 4, id, 65);
        }
        assert_eq!(section.biome.bits_per_entry(), 7);
        assert!(matches!(section.biome, PaletteData::Direct { .. }));
        assert_eq!(section.get_biome(1, 2, 3), 40);
        assert_eq!(section.get_biome(3, 3, 3), 57);

        for x in 0..4 {
            for y in 0..4 {
                for z in 0..4 {
                    section.set_biome(x, y, z, 3, 65);
                }
            }
        }
        assert_eq!(section.biome, PaletteData::SingleValue(3));
    }

    #[test]
    fn block_section_golden_bytes() {
        let mut section = ChunkSection::filled(1, 5);
        section.set_block_state(1, 0, 0, 9);
        let bytes = section.encode_to_bytes();
        let mut expected = vec![0x10, 0x00, 0x00, 0x00, 4, 2, 1, 9];
        expected.extend((1u64 << 4).to_be_bytes());
        expected.extend([0u8; 8 * 255]);
        expected.extend([0, 5]);
        assert_eq!(bytes, expected);

        let mut truncated = ChunkSection::filled(1, 5);
        truncated.block_state = PaletteData::Indirect {
            bits_per_entry: 4,
            palette: vec![1],
            data: vec![0x20],
        };
        assert_eq!(truncated.get_block_state(0, 0, 0), 1);
        assert_eq!(truncated.get_block_state(1, 0, 0), 0);
        assert_eq!(truncated.get_block_state(0, 15, 15), 1);
    }

    #[test]
    fn biome_bytes_match_paletted_container_layout() {
        let single = ChunkSection::filled(1, 5).encode_biomes_to_bytes();
        assert_eq!(single, [0, 5]);

        let mut section = ChunkSection::empty();
        section.biome = biome_palette(&cells(|i| if i == 1 { 9 } else { 4 }), 65);
        assert_eq!(
            section.encode_biomes_to_bytes(),
            [vec![1, 2, 4, 9], 2u64.to_be_bytes().to_vec()].concat()
        );

        section.biome = biome_palette(&cells(|i| i as i32), 65);
        let bytes = section.encode_biomes_to_bytes();
        assert_eq!(bytes[0], 7);
        assert_eq!(bytes.len(), 1 + 8 * 8);
        let first = u64::from_be_bytes(bytes[1..9].try_into().unwrap());
        assert_eq!(first & 0x7F, 0);
        assert_eq!((first >> 7) & 0x7F, 1);
        assert_eq!((first >> 56) & 0x7F, 8);
    }

    #[test]
    fn chunks_biomes_writes_z_before_x_and_only_biome_containers() {
        let mut sections: Vec<ChunkSection> = (0..2).map(|_| ChunkSection::filled(1, 5)).collect();
        sections[1].set_biome(0, 0, 0, 6, 65);
        let packet = ChunksBiomes {
            chunks: vec![ChunkBiomeData::from_sections(3, -2, &sections)],
        };
        let mut bytes = Vec::new();
        PlayPacket::ChunksBiomes(packet.clone()).encode(&mut bytes);

        let mut expected = vec![0x0D, 1];
        expected.extend((-2i32).to_be_bytes());
        expected.extend(3i32.to_be_bytes());
        let mut blob = vec![0, 5];
        blob.extend([1, 2, 6, 5]);
        blob.extend((!1u64).to_be_bytes());
        expected.push(blob.len() as u8);
        expected.extend(&blob);
        assert_eq!(bytes, expected);

        let mut slice = &bytes[1..];
        assert_eq!(ChunksBiomes::decode(&mut slice).unwrap(), packet);
        assert!(slice.is_empty());
    }

    #[test]
    fn builder_biomes_per_cell_keep_uniform_chunks_single_valued() {
        let uniform = ChunkBuilder::new(0, 0).biome(7).build();
        assert!(
            uniform
                .sections
                .iter()
                .all(|s| s.biome == PaletteData::SingleValue(7))
        );

        let varied = ChunkBuilder::new(2, -1)
            .biomes_from(|x, y, z| if y >= 64 && x >= 40 && z < -8 { 1 } else { 0 })
            .build();
        assert_eq!(varied.sections[7].biome, PaletteData::SingleValue(0));
        let top = &varied.sections[8];
        assert_eq!(top.get_biome(0, 0, 0), 0);
        assert_eq!(top.get_biome(2, 0, 0), 1);
        assert_eq!(top.get_biome(2, 0, 2), 0);
        assert_eq!(top.biome.bits_per_entry(), 1);
    }
}
