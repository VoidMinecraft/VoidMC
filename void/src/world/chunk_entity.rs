use std::collections::HashMap;

use bevy_ecs::prelude::*;
use voidmc_protocol::clientbound::chunk::{
    blocks, Chunk as ProtocolChunk, ChunkDataAndLight, ChunkHeightmaps, ChunkSection, LightData,
    PaletteData,
};

use super::chunk_pos::ChunkPos;
use super::dimension::DimensionId;

/// The chunk's column position. Component on chunk entities.
#[derive(Component)]
pub struct ChunkPosition(pub ChunkPos);

/// The chunk's section/block data. Component on chunk entities.
#[derive(Component)]
pub struct ChunkData {
    pub sections: Vec<ChunkSection>,
    pub heightmaps: ChunkHeightmaps,
    pub light: LightData,
}

/// World y range covered by `ChunkData::sections`.
pub const CHUNK_MIN_Y: i32 = -64;
pub const CHUNK_MAX_Y: i32 = 319;

impl ChunkData {
    /// Creates ChunkData from a protocol Chunk, consuming its data.
    pub fn from_protocol_chunk(chunk: &ProtocolChunk) -> Self {
        Self {
            sections: chunk.sections.clone(),
            heightmaps: chunk.heightmaps.clone(),
            light: chunk.light.clone(),
        }
    }

    /// Reads the block-state id at the given local-x, world-y, local-z. Returns
    /// `None` if the y is outside the chunk's vertical range.
    pub fn get_block(&self, local_x: u8, world_y: i32, local_z: u8) -> Option<i32> {
        let (section_idx, local_y) = world_y_to_section(world_y)?;
        let section = self.sections.get(section_idx)?;
        Some(section.get_block_state(local_x, local_y, local_z))
    }

    /// Writes a block-state id at the given local-x, world-y, local-z and
    /// returns the previous value. Returns `None` if y is outside range.
    pub fn set_block(
        &mut self,
        local_x: u8,
        world_y: i32,
        local_z: u8,
        block_state_id: i32,
    ) -> Option<i32> {
        let (section_idx, local_y) = world_y_to_section(world_y)?;
        let section = self.sections.get_mut(section_idx)?;
        Some(section.set_block_state(local_x, local_y, local_z, block_state_id))
    }

    /// Converts this chunk data into a ChunkDataAndLight packet.
    pub fn to_packet(&self, x: i32, z: i32) -> ChunkDataAndLight {
        let mut data = Vec::new();
        for section in &self.sections {
            data.extend(section.encode_to_bytes());
        }

        ChunkDataAndLight {
            chunk_x: x,
            chunk_z: z,
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

fn world_y_to_section(world_y: i32) -> Option<(usize, u8)> {
    if !(CHUNK_MIN_Y..=CHUNK_MAX_Y).contains(&world_y) {
        return None;
    }
    let shifted = (world_y - CHUNK_MIN_Y) as u32;
    Some(((shifted / 16) as usize, (shifted % 16) as u8))
}

/// Marker on chunk entities that have been mutated since they were last
/// persisted. `WorldSerialization` can read this to drive incremental saves.
#[derive(Component, Default)]
pub struct ChunkDirty;

/// Which dimension this chunk belongs to. Component on chunk entities.
#[derive(Component)]
pub struct ChunkDimension(pub DimensionId);

/// Spatial index: maps (dimension, chunk_pos) -> Entity for O(1) lookup.
#[derive(Resource, Default)]
pub struct ChunkIndex(pub HashMap<(DimensionId, ChunkPos), Entity>);

/// Returns true when a block state should be treated as solid for simple entity collision.
pub fn is_solid_block_state(block_state: i32) -> bool {
    block_state != blocks::AIR && block_state != blocks::WATER
}

/// Reads a block state from a section at local coordinates.
fn block_state_in_section(section: &ChunkSection, local_x: usize, local_y: usize, local_z: usize) -> i32 {
    match &section.block_state {
        PaletteData::SingleValue(id) => *id,
        PaletteData::Indirect {
            bits_per_entry,
            palette,
            data,
        } => {
            let bits = *bits_per_entry as usize;
            let block_index = local_y * 256 + local_z * 16 + local_x;
            let bit_index = block_index * bits;
            let long_idx = bit_index / 64;
            let bit_offset = bit_index % 64;
            let mask = if bits == 64 { u64::MAX } else { (1u64 << bits) - 1 };

            let raw = if bit_offset + bits <= 64 {
                (data.get(long_idx).copied().unwrap_or(0) >> bit_offset) & mask
            } else {
                let low = data.get(long_idx).copied().unwrap_or(0) >> bit_offset;
                let high = data.get(long_idx + 1).copied().unwrap_or(0) << (64 - bit_offset);
                (low | high) & mask
            };

            palette.get(raw as usize).copied().unwrap_or(blocks::AIR)
        }
    }
}

/// Returns the block state at the given world coordinate, if the chunk is loaded.
pub fn block_state_at_world(
    chunk_index: &ChunkIndex,
    chunks: &Query<(&ChunkPosition, &ChunkData)>,
    dimension: DimensionId,
    world_x: i32,
    world_y: i32,
    world_z: i32,
) -> Option<i32> {
    let section_y = world_y + 64;
    if !(0..384).contains(&section_y) {
        return None;
    }

    let chunk_pos = ChunkPos::new(world_x.div_euclid(16), world_z.div_euclid(16));
    let entity = chunk_index.0.get(&(dimension, chunk_pos))?;
    let (_, chunk_data) = chunks.get(*entity).ok()?;

    let section_idx = (section_y / 16) as usize;
    let local_y = (section_y % 16) as usize;
    let local_x = world_x.rem_euclid(16) as usize;
    let local_z = world_z.rem_euclid(16) as usize;

    let section = chunk_data.sections.get(section_idx)?;
    Some(block_state_in_section(section, local_x, local_y, local_z))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_chunk_data() -> ChunkData {
        ChunkData {
            sections: (0..24).map(|_| ChunkSection::empty()).collect(),
            heightmaps: ChunkHeightmaps::empty(),
            light: LightData::empty(),
        }
    }

    #[test]
    fn set_and_get_block_at_negative_y() {
        let mut data = empty_chunk_data();
        let prev = data.set_block(2, -60, 5, 1).expect("y in range");
        assert_eq!(prev, 0);
        assert_eq!(data.get_block(2, -60, 5), Some(1));
    }

    #[test]
    fn set_block_returns_none_below_min_y() {
        let mut data = empty_chunk_data();
        assert_eq!(data.set_block(0, -65, 0, 1), None);
        assert_eq!(data.get_block(0, -65, 0), None);
    }

    #[test]
    fn set_block_returns_none_above_max_y() {
        let mut data = empty_chunk_data();
        assert_eq!(data.set_block(0, 320, 0, 1), None);
        assert_eq!(data.get_block(0, 320, 0), None);
    }

    #[test]
    fn world_y_to_section_maps_section_boundaries() {
        assert_eq!(world_y_to_section(-64), Some((0, 0)));
        assert_eq!(world_y_to_section(-49), Some((0, 15)));
        assert_eq!(world_y_to_section(-48), Some((1, 0)));
        assert_eq!(world_y_to_section(0), Some((4, 0)));
        assert_eq!(world_y_to_section(319), Some((23, 15)));
    }
}
