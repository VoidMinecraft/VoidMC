use bevy_ecs::prelude::*;
use void_protocol::clientbound::chunk::{
    blocks, ChunkBuilder, Chunk as ProtocolChunk,
};

use super::chunk_pos::ChunkPos;

/// Pluggable terrain generator trait.
pub trait WorldGenerator: Send + Sync {
    /// Generates a full chunk at the given position.
    fn generate_chunk(&self, pos: &ChunkPos) -> ProtocolChunk;

    /// Returns the terrain surface Y at a given block coordinate.
    fn surface_height_at(&self, block_x: i32, block_z: i32) -> i32;
}

/// Default sine-wave terrain generator (matches original behavior).
pub struct DefaultWorldGenerator {
    pub base_height: i32,
    pub frequency: f64,
    pub amplitude: f64,
    pub water_level: i32,
}

impl Default for DefaultWorldGenerator {
    fn default() -> Self {
        Self {
            base_height: BASE_HEIGHT,
            frequency: FREQUENCY,
            amplitude: AMPLITUDE,
            water_level: 62,
        }
    }
}

impl WorldGenerator for DefaultWorldGenerator {
    fn generate_chunk(&self, pos: &ChunkPos) -> ProtocolChunk {
        let base = self.base_height;
        let freq = self.frequency;
        let amp = self.amplitude;
        let water = self.water_level;
        ChunkBuilder::new(pos.x, pos.z)
            .with_heightmap_layered(
                |x, z| {
                    let main_wave =
                        (x as f64 * freq).sin() + (z as f64 * freq).sin();
                    let detail = (x as f64 * freq * 3.7).sin() * 0.3
                        + (z as f64 * freq * 2.9).sin() * 0.3;
                    base + ((main_wave + detail) * amp) as i32
                },
                &[
                    (1, blocks::GRASS_BLOCK),
                    (4, blocks::DIRT),
                    (i32::MAX, blocks::STONE),
                ],
            )
            .add_water(water)
            .build()
    }

    fn surface_height_at(&self, block_x: i32, block_z: i32) -> i32 {
        let main_wave =
            (block_x as f64 * self.frequency).sin() + (block_z as f64 * self.frequency).sin();
        let detail = (block_x as f64 * self.frequency * 3.7).sin() * 0.3
            + (block_z as f64 * self.frequency * 2.9).sin() * 0.3;
        self.base_height + ((main_wave + detail) * self.amplitude) as i32
    }
}

/// Bevy resource wrapping the active world generator.
#[derive(Resource)]
pub struct WorldGen(pub Box<dyn WorldGenerator>);

const BASE_HEIGHT: i32 = 64;
const FREQUENCY: f64 = 0.02;
const AMPLITUDE: f64 = 8.0;

/// Convenience: computes the terrain surface Y using `DefaultWorldGenerator`.
pub fn surface_height_at(block_x: i32, block_z: i32) -> i32 {
    DefaultWorldGenerator::default().surface_height_at(block_x, block_z)
}

/// Convenience: generates a terrain chunk using `DefaultWorldGenerator`.
pub fn generate_chunk(pos: &ChunkPos) -> ProtocolChunk {
    DefaultWorldGenerator::default().generate_chunk(pos)
}

/// Generates all chunks in a square radius around a center position.
pub fn generate_spawn_area(center: ChunkPos, radius: i32) -> Vec<(ChunkPos, ProtocolChunk)> {
    center
        .chunks_in_radius(radius)
        .into_iter()
        .map(|pos| {
            let chunk = generate_chunk(&pos);
            (pos, chunk)
        })
        .collect()
}
