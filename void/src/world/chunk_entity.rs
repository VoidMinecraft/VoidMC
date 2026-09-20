use std::collections::HashMap;

use bevy_ecs::prelude::*;
use tracing::instrument;
use voidmc_protocol::clientbound::chunk::{
    Chunk as ProtocolChunk, ChunkDataAndLight, ChunkHeightmaps, ChunkSection, LightData, blocks,
};
use voidmc_protocol::types::BlockPosition;

use super::block_entity::{BlockEntity, BlockEntityError, BlockEntityKind};
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
    block_entities: HashMap<LocalBlockPos, BlockEntity>,
    pending_block_entities: HashMap<LocalBlockPos, BlockEntityKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct LocalBlockPos {
    x: u8,
    y: i16,
    z: u8,
}

impl LocalBlockPos {
    fn of(position: BlockPosition) -> Self {
        Self {
            x: position.x.rem_euclid(16) as u8,
            y: position.y,
            z: position.z.rem_euclid(16) as u8,
        }
    }

    fn in_chunk(self, chunk: ChunkPos) -> BlockPosition {
        BlockPosition {
            x: chunk.x * 16 + self.x as i32,
            y: self.y,
            z: chunk.z * 16 + self.z as i32,
        }
    }
}

/// World y range covered by `ChunkData::sections`.
pub const CHUNK_MIN_Y: i32 = -64;
pub const CHUNK_MAX_Y: i32 = 319;

impl ChunkData {
    /// Creates ChunkData from raw section/heightmap/light data.
    ///
    /// Useful for constructing chunks outside the world generator, e.g. when an
    /// external persistence layer deserializes a chunk from disk.
    pub fn new(sections: Vec<ChunkSection>, heightmaps: ChunkHeightmaps, light: LightData) -> Self {
        Self {
            sections,
            heightmaps,
            light,
            block_entities: HashMap::new(),
            pending_block_entities: HashMap::new(),
        }
    }

    /// Creates ChunkData from a protocol Chunk, consuming its data.
    #[instrument(name = "chunk_data_conversion", level = "info", skip(chunk))]
    pub fn from_protocol_chunk(chunk: &ProtocolChunk) -> Self {
        Self::new(
            chunk.sections.clone(),
            chunk.heightmaps.clone(),
            chunk.light.clone(),
        )
    }

    pub fn block_entity(&self, position: BlockPosition) -> Option<&BlockEntity> {
        self.block_entities.get(&LocalBlockPos::of(position))
    }

    /// Every block entity with its world position; `chunk` is this chunk's column.
    pub fn block_entities(
        &self,
        chunk: ChunkPos,
    ) -> impl Iterator<Item = (BlockPosition, &BlockEntity)> + '_ {
        self.block_entities
            .iter()
            .map(move |(local, block_entity)| (local.in_chunk(chunk), block_entity))
    }

    /// Stores `block_entity` at `position` and returns the one it replaced.
    /// Fails unless the block at `position` hosts that kind, so a sign's NBT
    /// can never sit on a stone block.
    pub fn set_block_entity(
        &mut self,
        position: BlockPosition,
        block_entity: impl Into<BlockEntity>,
    ) -> Result<Option<BlockEntity>, BlockEntityError> {
        let block_entity = block_entity.into();
        let local = LocalBlockPos::of(position);
        let block_state = self
            .get_block(local.x, local.y as i32, local.z)
            .ok_or(BlockEntityError::OutsideWorld)?;
        if !block_entity.kind().hosted_by(block_state) {
            return Err(BlockEntityError::WrongBlock {
                block_state,
                kind: block_entity.kind(),
            });
        }
        self.pending_block_entities
            .insert(local, block_entity.kind());
        Ok(self.block_entities.insert(local, block_entity))
    }

    /// Loads persisted block entities without validating or queueing updates.
    pub fn restore_block_entities(
        &mut self,
        block_entities: impl IntoIterator<Item = (BlockPosition, BlockEntity)>,
    ) {
        self.block_entities.extend(
            block_entities
                .into_iter()
                .map(|(position, block_entity)| (LocalBlockPos::of(position), block_entity)),
        );
    }

    pub fn remove_block_entity(&mut self, position: BlockPosition) -> Option<BlockEntity> {
        let local = LocalBlockPos::of(position);
        let removed = self.block_entities.remove(&local)?;
        self.pending_block_entities.insert(local, removed.kind());
        Some(removed)
    }

    /// Positions changed since the last sync, with the kind to reset when the
    /// block entity is gone. Draining it is the sync system's job.
    pub fn take_pending_block_entities(
        &mut self,
        chunk: ChunkPos,
    ) -> Vec<(BlockPosition, BlockEntityKind)> {
        std::mem::take(&mut self.pending_block_entities)
            .into_iter()
            .map(|(local, kind)| (local.in_chunk(chunk), kind))
            .collect()
    }

    pub fn has_pending_block_entities(&self) -> bool {
        !self.pending_block_entities.is_empty()
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
        let previous = section.set_block_state(local_x, local_y, local_z, block_state_id);
        if previous != block_state_id && !self.block_entities.is_empty() {
            let local = LocalBlockPos {
                x: local_x,
                y: world_y as i16,
                z: local_z,
            };
            let stale = self
                .block_entities
                .get(&local)
                .is_some_and(|block_entity| !block_entity.kind().hosted_by(block_state_id));
            if stale {
                self.block_entities.remove(&local);
                self.pending_block_entities.remove(&local);
            }
        }
        Some(previous)
    }

    /// Converts this chunk data into a ChunkDataAndLight packet.
    #[instrument(name = "chunk_packet_encoding", level = "info", skip(self))]
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
            block_entities: self
                .block_entities
                .iter()
                .map(|(local, block_entity)| {
                    block_entity.chunk_entry(local.in_chunk(ChunkPos::new(x, z)))
                })
                .collect(),
            sky_light_mask: self.light.sky_light_mask.clone(),
            block_light_mask: self.light.block_light_mask.clone(),
            empty_sky_light_mask: self.light.empty_sky_light_mask.clone(),
            empty_block_light_mask: self.light.empty_block_light_mask.clone(),
            sky_light_arrays: self.light.sky_light_arrays.clone(),
            block_light_arrays: self.light.block_light_arrays.clone(),
        }
    }
}

pub(crate) fn world_y_to_section(world_y: i32) -> Option<(usize, u8)> {
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
fn block_state_in_section(
    section: &ChunkSection,
    local_x: usize,
    local_y: usize,
    local_z: usize,
) -> i32 {
    section.get_block_state(local_x as u8, local_y as u8, local_z as u8)
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
        ChunkData::new(
            (0..24).map(|_| ChunkSection::empty()).collect(),
            ChunkHeightmaps::empty(),
            LightData::empty(),
        )
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
