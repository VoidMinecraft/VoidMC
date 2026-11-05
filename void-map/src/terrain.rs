//! Samples a world generator over a view.

use voidmc::{BiomeId, WorldGenerator};

use crate::window::Window;

/// Biomes are stored per 4x4x4 cell; `cell_biome` expects its lowest corner.
const BIOME_CELL_MASK: i32 = !3;

/// Surface heights and biomes of every cell of a view.
///
/// Heights include one extra column to the west and row to the north, so a
/// cell on the edge of the view shades like the same cell further inside.
pub struct Terrain {
    window: Window,
    heights: Vec<i32>,
    biomes: Vec<BiomeId>,
}

impl Terrain {
    pub fn sample(generator: &dyn WorldGenerator, window: &Window) -> Self {
        let (cols, rows) = (i32::from(window.cols()), i32::from(window.rows()));
        let scale = i32::from(window.scale());
        let (left, top) = window.block_at(0, 0);
        let mut heights = Vec::with_capacity(((cols + 1) * (rows + 1)) as usize);
        let mut biomes = Vec::with_capacity((cols * rows) as usize);
        for row in -1..rows {
            for col in -1..cols {
                let (x, z) = (left + col * scale, top + row * scale);
                let height = generator.surface_height_at(x, z);
                heights.push(height);
                if row >= 0 && col >= 0 {
                    biomes.push(generator.cell_biome(
                        x & BIOME_CELL_MASK,
                        height & BIOME_CELL_MASK,
                        z & BIOME_CELL_MASK,
                    ));
                }
            }
        }
        Self {
            window: *window,
            heights,
            biomes,
        }
    }

    pub fn window(&self) -> &Window {
        &self.window
    }

    /// Surface height of a cell of the view.
    pub fn height(&self, col: u16, row: u16) -> i32 {
        self.padded_height(usize::from(col) + 1, usize::from(row) + 1)
    }

    /// Height of the cell one step west of a cell of the view.
    pub fn height_west(&self, col: u16, row: u16) -> i32 {
        self.padded_height(usize::from(col), usize::from(row) + 1)
    }

    /// Height of the cell one step north of a cell of the view.
    pub fn height_north(&self, col: u16, row: u16) -> i32 {
        self.padded_height(usize::from(col) + 1, usize::from(row))
    }

    fn padded_height(&self, col: usize, row: usize) -> i32 {
        self.heights[row * (usize::from(self.window.cols()) + 1) + col]
    }

    pub fn biome(&self, col: u16, row: u16) -> BiomeId {
        self.biomes[usize::from(row) * usize::from(self.window.cols()) + usize::from(col)]
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use voidmc::{ChunkPos, DefaultWorldGenerator};
    use voidmc_protocol::clientbound::Chunk;

    use super::*;
    use crate::testing::{Flat, window};

    #[test]
    fn heights_come_from_the_generator_at_each_cell() {
        let generator = DefaultWorldGenerator::default();
        let view = window(100, -60, 12, 9, 4);
        let terrain = Terrain::sample(&generator, &view);
        for row in 0..9 {
            for col in 0..12 {
                let (x, z) = view.block_at(col, row);
                assert_eq!(terrain.height(col, row), generator.surface_height_at(x, z));
                assert_eq!(
                    terrain.height_west(col, row),
                    generator.surface_height_at(x - 4, z)
                );
                assert_eq!(
                    terrain.height_north(col, row),
                    generator.surface_height_at(x, z - 4)
                );
            }
        }
    }

    #[test]
    fn the_generator_is_injected() {
        let desert = BiomeId::named("desert").unwrap();
        let terrain = Terrain::sample(&Flat::new(90, desert), &window(0, 0, 3, 2, 4));
        assert_eq!(terrain.height(2, 1), 90);
        assert_eq!(terrain.biome(2, 1), desert);
    }

    struct Recording(Mutex<Vec<(i32, i32, i32)>>);

    impl WorldGenerator for Recording {
        fn generate_chunk(&self, pos: &ChunkPos) -> Chunk {
            Flat::new(0, BiomeId::plains()).generate_chunk(pos)
        }

        fn surface_height_at(&self, _x: i32, _z: i32) -> i32 {
            71
        }

        fn cell_biome(&self, x: i32, y: i32, z: i32) -> BiomeId {
            self.0.lock().unwrap().push((x, y, z));
            BiomeId::plains()
        }
    }

    #[test]
    fn biomes_are_read_at_the_surface_on_cell_corners() {
        let generator = Recording(Mutex::new(Vec::new()));
        Terrain::sample(&generator, &window(-7, 5, 2, 2, 3));
        let calls = generator.0.into_inner().unwrap();
        assert_eq!(calls.len(), 4);
        for (x, y, z) in calls {
            assert_eq!((x % 4, y, z % 4), (0, 68, 0));
        }
    }
}
