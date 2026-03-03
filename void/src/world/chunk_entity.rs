use std::collections::HashMap;

use bevy_ecs::prelude::*;
use void_protocol::clientbound::chunk::{
    ChunkHeightmaps, ChunkSection, LightData, Chunk as ProtocolChunk, ChunkDataAndLight,
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

impl ChunkData {
    /// Creates ChunkData from a protocol Chunk, consuming its data.
    pub fn from_protocol_chunk(chunk: &ProtocolChunk) -> Self {
        Self {
            sections: chunk.sections.clone(),
            heightmaps: chunk.heightmaps.clone(),
            light: chunk.light.clone(),
        }
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

/// Which dimension this chunk belongs to. Component on chunk entities.
#[derive(Component)]
pub struct ChunkDimension(pub DimensionId);

/// Spatial index: maps (dimension, chunk_pos) -> Entity for O(1) lookup.
#[derive(Resource, Default)]
pub struct ChunkIndex(pub HashMap<(DimensionId, ChunkPos), Entity>);
