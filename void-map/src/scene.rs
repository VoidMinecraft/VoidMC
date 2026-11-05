//! The block model of the world.

use std::num::{NonZeroU8, NonZeroU16};

use voidmc::BiomeId;
use voidmc_data::v26_1_2::blocks as b;
use voidmc_protocol::clientbound::{Chunk, ChunkBuilder};

use crate::terrain::Terrain;
use crate::window::Window;

/// Model radius in blocks. A round model fits a client's round view distance:
/// 7 chunks here, plus the camera's offset, are within 12.
pub const RADIUS: i32 = 112;
const SIZE: u16 = RADIUS as u16 * 2;
/// World blocks per model block. Height stays 1:1, which exaggerates relief.
pub const BLOCKS_PER_CELL: u8 = 4;
/// Height of the sea on the model, and of its underside.
pub const SEA_Y: i32 = 64;
const FLOOR_Y: i32 = 40;
const MAX_Y: i32 = 300;

/// Blocks above sea level where land turns to rock, then to snow.
const ROCK_LINE: i32 = 48;
const SNOW_LINE: i32 = 72;
const BEACH_LINE: i32 = 1;

/// The world window the model shows: all of it, centred on the origin.
pub fn window() -> Window {
    Window::new(
        0,
        0,
        NonZeroU16::new(SIZE).unwrap_or(NonZeroU16::MIN),
        NonZeroU16::new(SIZE).unwrap_or(NonZeroU16::MIN),
        NonZeroU8::new(BLOCKS_PER_CELL).unwrap_or(NonZeroU8::MIN),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Column {
    /// Highest solid block.
    ground: i32,
    surface: i32,
}

impl Column {
    /// Highest block, solid or water.
    fn top(self) -> i32 {
        self.ground.max(SEA_Y)
    }
}

/// Columns of a round block model. Coordinates are in blocks from its centre.
pub struct Scene {
    columns: Vec<Option<Column>>,
}

fn surface_block(biome: BiomeId, elevation: i32) -> i32 {
    let is = |names: &[&str]| names.iter().any(|name| BiomeId::named(name) == Some(biome));
    if elevation <= 0 {
        b::SAND
    } else if elevation >= SNOW_LINE || is(&["snowy_plains", "snowy_taiga", "ice_spikes"]) {
        b::SNOW_BLOCK
    } else if elevation >= ROCK_LINE || is(&["stony_shore"]) {
        b::STONE
    } else if elevation <= BEACH_LINE || is(&["desert", "beach"]) {
        b::SAND
    } else if is(&["badlands"]) {
        b::RED_SAND
    } else {
        b::GRASS_BLOCK
    }
}

impl Scene {
    /// `terrain` must come from [`window`]: the model has one block per cell.
    pub fn new(terrain: &Terrain, sea_level: i32) -> Self {
        let mut columns = Vec::with_capacity(usize::from(SIZE) * usize::from(SIZE));
        for row in 0..SIZE {
            for col in 0..SIZE {
                let (x, z) = (i32::from(col) - RADIUS, i32::from(row) - RADIUS);
                if x * x + z * z >= RADIUS * RADIUS {
                    columns.push(None);
                    continue;
                }
                let elevation = terrain.height(col, row) - sea_level;
                columns.push(Some(Column {
                    ground: (SEA_Y + elevation).clamp(FLOOR_Y + 1, MAX_Y),
                    surface: surface_block(terrain.biome(col, row), elevation),
                }));
            }
        }
        Self { columns }
    }

    fn column(&self, x: i32, z: i32) -> Option<Column> {
        if !(-RADIUS..RADIUS).contains(&x) || !(-RADIUS..RADIUS).contains(&z) {
            return None;
        }
        let index = (z + RADIUS) as usize * usize::from(SIZE) + (x + RADIUS) as usize;
        self.columns[index]
    }

    /// Highest block of a column, or `None` off the model.
    pub fn top(&self, x: i32, z: i32) -> Option<i32> {
        self.column(x, z).map(Column::top)
    }

    /// The model cell a world block falls on.
    pub fn cell_of(window: &Window, x: i32, z: i32) -> Option<(i32, i32)> {
        let (col, row) = window.cell_at(x, z)?;
        Some((i32::from(col) - RADIUS, i32::from(row) - RADIUS))
    }

    /// The chunks of the model, centred on chunk `(0, 0)`, with a ring of empty
    /// ones: the client only draws a chunk once its neighbours are there.
    pub fn chunks(&self, biome: BiomeId, biome_count: usize) -> Vec<Chunk> {
        let reach = RADIUS / 16 + 1;
        let mut built = Vec::new();
        for chunk_z in -reach..reach {
            for chunk_x in -reach..reach {
                built.push(self.chunk(chunk_x, chunk_z, biome, biome_count));
            }
        }
        built
    }

    fn chunk(&self, chunk_x: i32, chunk_z: i32, biome: BiomeId, count: usize) -> Chunk {
        let mut builder = ChunkBuilder::new(chunk_x, chunk_z)
            .biome(biome.0)
            .biome_registry_size(count);
        for local_z in 0..16u8 {
            for local_x in 0..16u8 {
                let (x, z) = (
                    chunk_x * 16 + i32::from(local_x),
                    chunk_z * 16 + i32::from(local_z),
                );
                let Some(column) = self.column(x, z) else {
                    continue;
                };
                for y in FLOOR_Y..column.ground {
                    builder.set_block(local_x, y, local_z, b::STONE);
                }
                builder.set_block(local_x, column.ground, local_z, column.surface);
                for y in column.ground + 1..=SEA_Y {
                    builder.set_block(local_x, y, local_z, b::WATER);
                }
            }
        }
        builder.build()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use voidmc::{DefaultWorldGenerator, WorldGenerator};

    use super::*;
    use crate::testing::Flat;

    const SEA: i32 = 62;

    fn scene(generator: &dyn WorldGenerator) -> Scene {
        Scene::new(&Terrain::sample(generator, &window()), SEA)
    }

    #[test]
    fn the_model_is_round() {
        let scene = scene(&Flat::new(70, BiomeId::plains()));
        assert!(scene.top(0, 0).is_some());
        assert!(scene.top(RADIUS - 1, 0).is_some());
        assert!(scene.top(RADIUS - 1, RADIUS - 1).is_none());
        assert!(scene.top(RADIUS, 0).is_none());
    }

    #[test]
    fn heights_follow_the_terrain_around_the_sea() {
        assert_eq!(
            scene(&Flat::new(70, BiomeId::plains())).top(0, 0),
            Some(SEA_Y + 8)
        );
        assert_eq!(
            scene(&Flat::new(50, BiomeId::plains())).top(0, 0),
            Some(SEA_Y)
        );
    }

    #[test]
    fn extreme_terrain_stays_inside_the_dimension() {
        let high = scene(&Flat::new(10_000, BiomeId::plains()));
        assert_eq!(high.top(0, 0), Some(MAX_Y));
        let deep = scene(&Flat::new(-10_000, BiomeId::plains()));
        assert_eq!(deep.column(0, 0).unwrap().ground, FLOOR_Y + 1);
    }

    #[test]
    fn the_surface_depends_on_elevation_and_biome() {
        let plains = BiomeId::plains();
        let desert = BiomeId::named("desert").unwrap();
        assert_eq!(surface_block(plains, 10), b::GRASS_BLOCK);
        assert_eq!(surface_block(desert, 10), b::SAND);
        assert_eq!(surface_block(plains, -3), b::SAND);
        assert_eq!(surface_block(plains, ROCK_LINE), b::STONE);
        assert_eq!(surface_block(plains, SNOW_LINE), b::SNOW_BLOCK);
    }

    #[test]
    fn every_chunk_of_the_model_has_all_its_neighbours() {
        let scene = scene(&DefaultWorldGenerator::default());
        let chunks: HashSet<_> = scene
            .chunks(BiomeId::plains(), 64)
            .iter()
            .map(|c| (c.x, c.z))
            .collect();
        for x in -RADIUS / 16 - 1..=RADIUS / 16 {
            for z in -RADIUS / 16 - 1..=RADIUS / 16 {
                let is_model = (x * 16..x * 16 + 16)
                    .any(|bx| (z * 16..z * 16 + 16).any(|bz| scene.top(bx, bz).is_some()));
                if is_model {
                    for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                        assert!(chunks.contains(&(x + dx, z + dz)), "{x} {z}");
                    }
                }
            }
        }
    }
}
