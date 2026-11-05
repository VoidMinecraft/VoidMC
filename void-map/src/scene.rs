//! The block model of the world: a small floating island of flat colours.

use std::num::{NonZeroU8, NonZeroU16};

use voidmc::BiomeId;
use voidmc_data::v26_1_2::blocks as b;
use voidmc_protocol::clientbound::{Chunk, ChunkBuilder};

use crate::pointer::CAMERA_Z;
use crate::terrain::Terrain;
use crate::window::Window;

/// Widest radius of the island in blocks. See `view` for what bounds it.
pub const RADIUS: i32 = 56;
const SIZE: u16 = RADIUS as u16 * 2;
/// World blocks per model block.
pub const BLOCKS_PER_CELL: u8 = 8;
/// Height of the sea on the model.
pub const SEA_Y: i32 = 64;
const LOWEST_Y: i32 = 30;
pub const MAX_Y: i32 = 140;

/// World blocks of elevation per terrace, and blocks of model height per terrace.
/// A terrace rises more than the land it stands for: that is the relief.
const TERRACE_STEP: i32 = 3;
const TERRACE_RISE: i32 = 2;
/// Terraces from the sea to where land turns to dark grass, rock and snow.
const FOREST_TERRACE: i32 = 2;
const ROCK_TERRACE: i32 = 4;
const SNOW_TERRACE: i32 = 6;
const BEACH_LINE: i32 = 1;

/// Width of the band of land that keeps the sea from reaching the rim.
const SHORE_WIDTH: f64 = 3.0;
/// How far the underside drops per block from the rim, and at most.
const UNDERSIDE_SLOPE: f64 = 1.5;
const UNDERSIDE_DEPTH: i32 = 30;
/// Blocks under the surface where cliffs turn dark, then black.
const DARK_DEPTH: i32 = 9;
const BLACK_DEPTH: i32 = 15;

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
    /// Lowest solid block.
    bottom: i32,
}

impl Column {
    /// Highest block, solid or sea.
    fn top(self) -> i32 {
        self.ground.max(SEA_Y)
    }

    fn is_land(self) -> bool {
        self.ground >= SEA_Y
    }
}

/// Columns of a block model. Coordinates are in blocks from its centre.
pub struct Scene {
    columns: Vec<Option<Column>>,
}

/// How far the rim is from the centre in a direction: a circle, but not quite.
fn rim_radius(x: f64, z: f64) -> f64 {
    let angle = z.atan2(x);
    let wobble = (angle * 2.0 + 0.7).sin() * 0.03 + (angle * 5.0).sin() * 0.03;
    f64::from(RADIUS) * (0.93 + wobble)
}

/// Flat colours read as a map from far away, where textures turn to noise.
fn surface_block(biome: BiomeId, elevation: i32, terrace: i32) -> i32 {
    let is = |names: &[&str]| names.iter().any(|name| BiomeId::named(name) == Some(biome));
    if terrace >= SNOW_TERRACE || is(&["snowy_plains", "snowy_taiga", "ice_spikes"]) {
        b::WHITE_CONCRETE
    } else if terrace >= ROCK_TERRACE || is(&["stony_shore"]) {
        b::LIGHT_GRAY_CONCRETE
    } else if elevation <= BEACH_LINE || is(&["desert", "beach"]) {
        b::SMOOTH_SANDSTONE
    } else if is(&["badlands"]) {
        b::BROWN_TERRACOTTA
    } else if terrace >= FOREST_TERRACE {
        b::LIME_TERRACOTTA
    } else {
        b::LIME_CONCRETE
    }
}

fn cliff_block(depth: i32) -> i32 {
    if depth >= BLACK_DEPTH {
        b::BLACK_CONCRETE
    } else if depth >= DARK_DEPTH {
        b::GRAY_CONCRETE
    } else {
        b::LIGHT_GRAY_CONCRETE
    }
}

impl Scene {
    /// `terrain` must come from [`window`]: the model has one block per cell.
    pub fn new(terrain: &Terrain, sea_level: i32) -> Self {
        let mut columns = Vec::with_capacity(usize::from(SIZE) * usize::from(SIZE));
        for row in 0..SIZE {
            for col in 0..SIZE {
                let (x, z) = (i32::from(col) - RADIUS, i32::from(row) - RADIUS);
                let elevation = terrain.height(col, row) - sea_level;
                let biome = terrain.biome(col, row);
                columns.push(Self::column_at(x, z, elevation, biome));
            }
        }
        Self { columns }
    }

    fn column_at(x: i32, z: i32, elevation: i32, biome: BiomeId) -> Option<Column> {
        let (cx, cz) = (f64::from(x) + 0.5, f64::from(z) + 0.5);
        let from_rim = rim_radius(cx, cz) - cx.hypot(cz);
        if from_rim < 0.0 {
            return None;
        }
        let elevation = if from_rim < SHORE_WIDTH {
            elevation.max(BEACH_LINE)
        } else {
            elevation
        };
        let terrace = elevation.div_euclid(TERRACE_STEP);
        let ground = if elevation > 0 {
            SEA_Y + 1 + terrace * TERRACE_RISE
        } else {
            SEA_Y - 1
        };
        let depth = ((from_rim + 1.0) * UNDERSIDE_SLOPE) as i32;
        Some(Column {
            ground: ground.min(MAX_Y),
            surface: surface_block(biome, elevation, terrace),
            bottom: (SEA_Y - 2 - depth.min(UNDERSIDE_DEPTH)).max(LOWEST_Y),
        })
    }

    fn index(x: i32, z: i32) -> Option<usize> {
        let range = -RADIUS..RADIUS;
        (range.contains(&x) && range.contains(&z))
            .then(|| (z + RADIUS) as usize * usize::from(SIZE) + (x + RADIUS) as usize)
    }

    fn column(&self, x: i32, z: i32) -> Option<Column> {
        self.columns[Self::index(x, z)?]
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

    /// Lays a path between two cells, over land only.
    pub fn pave(&mut self, from: (i32, i32), to: (i32, i32)) {
        let steps = (to.0 - from.0).abs().max((to.1 - from.1).abs()).max(1);
        for step in 0..=steps {
            let along = |a: i32, b: i32| a + (b - a) * step / steps;
            let (x, z) = (along(from.0, to.0), along(from.1, to.1));
            let Some(index) = Self::index(x, z) else {
                continue;
            };
            if let Some(column) = &mut self.columns[index]
                && column.is_land()
            {
                column.surface = b::BROWN_CONCRETE;
            }
        }
    }

    /// The chunks of the model, centred on chunk `(0, 0)`, with a ring of empty
    /// ones: the client only draws a chunk once its neighbours are there. They run
    /// further south, under the camera: the engine drops an entity that leaves the
    /// chunks a player has, and a panning camera is an entity.
    pub fn chunks(&self, biome: BiomeId, biome_count: usize) -> Vec<Chunk> {
        let reach = (RADIUS + 15) / 16 + 1;
        let mut built = Vec::new();
        let south = (CAMERA_Z as i32 + RADIUS) / 16 + 2;
        for chunk_z in -reach..south {
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
                for y in column.bottom..column.ground {
                    builder.set_block(local_x, y, local_z, cliff_block(column.ground - y));
                }
                builder.set_block(local_x, column.ground, local_z, column.surface);
                for y in column.ground + 1..=SEA_Y {
                    builder.set_block(local_x, y, local_z, b::LIGHT_BLUE_CONCRETE);
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
    fn the_island_ends_inside_its_radius() {
        let scene = scene(&Flat::new(70, BiomeId::plains()));
        assert!(scene.top(0, 0).is_some());
        assert!(scene.top(RADIUS - 1, RADIUS - 1).is_none());
        assert!(scene.top(RADIUS, 0).is_none());
    }

    #[test]
    fn land_rises_by_terraces() {
        let height = |y| scene(&Flat::new(y, BiomeId::plains())).top(0, 0);
        assert_eq!(height(SEA + 3), height(SEA + 5));
        assert_eq!(height(SEA + 6), height(SEA + 3).map(|y| y + TERRACE_RISE));
    }

    #[test]
    fn the_sea_is_flat() {
        let scene = scene(&Flat::new(50, BiomeId::plains()));
        assert_eq!(scene.top(0, 0), Some(SEA_Y));
    }

    #[test]
    fn the_sea_never_spills_over_the_rim() {
        let scene = scene(&Flat::new(50, BiomeId::plains()));
        let rim = (0..RADIUS).rev().find(|x| scene.top(*x, 0).is_some());
        assert!(scene.column(rim.unwrap(), 0).unwrap().is_land());
    }

    #[test]
    fn extreme_terrain_stays_inside_the_dimension() {
        let high = scene(&Flat::new(10_000, BiomeId::plains()));
        assert_eq!(high.top(0, 0), Some(MAX_Y));
        let deep = scene(&Flat::new(-10_000, BiomeId::plains()));
        assert!(deep.column(0, 0).unwrap().bottom >= LOWEST_Y);
    }

    #[test]
    fn the_underside_is_thinner_at_the_rim() {
        let scene = scene(&Flat::new(70, BiomeId::plains()));
        let rim = (0..RADIUS).rev().find(|x| scene.top(*x, 0).is_some());
        let bottom = |x| scene.column(x, 0).unwrap().bottom;
        assert!(bottom(rim.unwrap()) > bottom(0));
    }

    #[test]
    fn the_surface_depends_on_elevation_and_biome() {
        let plains = BiomeId::plains();
        let desert = BiomeId::named("desert").unwrap();
        assert_eq!(surface_block(plains, 4, 1), b::LIME_CONCRETE);
        assert_eq!(
            surface_block(plains, 10, FOREST_TERRACE),
            b::LIME_TERRACOTTA
        );
        assert_eq!(surface_block(desert, 4, 1), b::SMOOTH_SANDSTONE);
        assert_eq!(surface_block(plains, 1, 0), b::SMOOTH_SANDSTONE);
        assert_eq!(
            surface_block(plains, 20, ROCK_TERRACE),
            b::LIGHT_GRAY_CONCRETE
        );
        assert_eq!(surface_block(plains, 30, SNOW_TERRACE), b::WHITE_CONCRETE);
    }

    #[test]
    fn cliffs_fade_to_black() {
        assert_eq!(cliff_block(1), b::LIGHT_GRAY_CONCRETE);
        assert_eq!(cliff_block(DARK_DEPTH), b::GRAY_CONCRETE);
        assert_eq!(cliff_block(BLACK_DEPTH), b::BLACK_CONCRETE);
    }

    #[test]
    fn paths_cross_land_and_not_the_sea() {
        let mut land = scene(&Flat::new(70, BiomeId::plains()));
        land.pave((-5, 0), (5, 3));
        assert_eq!(land.column(5, 3).unwrap().surface, b::BROWN_CONCRETE);
        assert_eq!(land.column(0, 1).unwrap().surface, b::BROWN_CONCRETE);

        let mut sea = scene(&Flat::new(50, BiomeId::plains()));
        let before = sea.column(0, 0);
        sea.pave((-5, 0), (5, 0));
        assert_eq!(sea.column(0, 0), before);
    }

    #[test]
    fn every_chunk_of_the_model_has_all_its_neighbours() {
        let scene = scene(&DefaultWorldGenerator::default());
        let chunks: HashSet<_> = scene
            .chunks(BiomeId::plains(), 64)
            .iter()
            .map(|c| (c.x, c.z))
            .collect();
        let reach = (RADIUS + 15) / 16;
        for x in -reach..reach {
            for z in -reach..reach {
                for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    assert!(chunks.contains(&(x + dx, z + dz)), "{x} {z}");
                }
            }
        }
    }
}
