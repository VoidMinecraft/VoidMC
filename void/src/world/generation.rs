use bevy_ecs::prelude::*;
use voidmc_protocol::clientbound::chunk::{Chunk as ProtocolChunk, ChunkBuilder, blocks};

use super::biome::BiomeId;
use super::chunk_pos::ChunkPos;

/// Pluggable terrain generator trait.
pub trait WorldGenerator: Send + Sync {
    /// Generates a full chunk at the given position.
    fn generate_chunk(&self, pos: &ChunkPos) -> ProtocolChunk;

    /// Returns the Y of the first air block above the terrain and any water at
    /// a given block coordinate: where an entity standing on the surface has
    /// its feet.
    fn surface_height_at(&self, block_x: i32, block_z: i32) -> i32;

    /// Biome of the 4×4×4 cell whose lowest corner is at these block
    /// coordinates. Feed it to [`ChunkBuilder::biomes_from`] in `generate_chunk`.
    fn cell_biome(&self, block_x: i32, block_y: i32, block_z: i32) -> BiomeId {
        let _ = (block_x, block_y, block_z);
        BiomeId::plains()
    }
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
            .biomes_from(|x, y, z| self.cell_biome(x, y, z).0)
            .with_heightmap_layered(
                |x, z| {
                    let main_wave = (x as f64 * freq).sin() + (z as f64 * freq).sin();
                    let detail =
                        (x as f64 * freq * 3.7).sin() * 0.3 + (z as f64 * freq * 2.9).sin() * 0.3;
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
        let terrain = self.base_height + ((main_wave + detail) * self.amplitude) as i32;
        terrain.max(self.water_level + 1)
    }
}

/// Bevy resource wrapping the active world generator.
#[derive(Resource)]
pub struct WorldGen(pub Box<dyn WorldGenerator>);

const BASE_HEIGHT: i32 = 64;
const FREQUENCY: f64 = 0.02;
const AMPLITUDE: f64 = 8.0;

/// [`WorldGenerator::surface_height_at`] of `DefaultWorldGenerator::default()`.
///
/// Always the default terrain, never the generator configured with
/// `ServerConfigBuilder::world_generator`: on a running server, ask the
/// [`WorldGen`] resource instead.
pub fn surface_height_at(block_x: i32, block_z: i32) -> i32 {
    DefaultWorldGenerator::default().surface_height_at(block_x, block_z)
}

/// [`WorldGenerator::generate_chunk`] of `DefaultWorldGenerator::default()`.
///
/// Always the default terrain, never the generator configured with
/// `ServerConfigBuilder::world_generator`: on a running server, ask the
/// [`WorldGen`] resource instead.
pub fn generate_chunk(pos: &ChunkPos) -> ProtocolChunk {
    DefaultWorldGenerator::default().generate_chunk(pos)
}

/// Generates the default terrain of all chunks in a square radius around a
/// center position.
#[deprecated(
    note = "always uses DefaultWorldGenerator, not the configured generator, and the server \
            does not call it: the spawn area is pre-generated from the `WorldGen` resource. \
            Iterate `center.chunks_in_radius(radius)` with your `WorldGenerator` instead"
)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::ChunkData;

    #[test]
    fn surface_height_is_the_first_air_block_above_terrain_and_water() {
        let generator = DefaultWorldGenerator::default();
        let mut underwater = 0;
        // (-5, -5) lies in the sine trough, below the water level.
        for pos in [
            ChunkPos::new(0, 0),
            ChunkPos::new(-5, -5),
            ChunkPos::new(3, -2),
        ] {
            let chunk = ChunkData::from_protocol_chunk(&generator.generate_chunk(&pos));
            for z in 0..16u8 {
                for x in 0..16u8 {
                    let (block_x, block_z) = (pos.x * 16 + i32::from(x), pos.z * 16 + i32::from(z));
                    let y = generator.surface_height_at(block_x, block_z);
                    let below = chunk.get_block(x, y - 1, z);
                    assert_eq!(
                        chunk.get_block(x, y, z),
                        Some(blocks::AIR),
                        "({block_x}, {y}, {block_z})"
                    );
                    assert_ne!(
                        below,
                        Some(blocks::AIR),
                        "({block_x}, {}, {block_z})",
                        y - 1
                    );
                    underwater += usize::from(below == Some(blocks::WATER));
                }
            }
        }
        assert!(
            underwater > 0,
            "no sampled column was below the water level"
        );
    }
}
