//! Fixtures shared by the unit tests.

use std::num::{NonZeroU8, NonZeroU16};

use voidmc::{BiomeId, ChunkPos, WorldGenerator};
use voidmc_protocol::clientbound::{Chunk, ChunkBuilder};

use crate::window::Window;

pub fn window(x: i32, z: i32, cols: u16, rows: u16, scale: u8) -> Window {
    Window::new(
        x,
        z,
        NonZeroU16::new(cols).unwrap(),
        NonZeroU16::new(rows).unwrap(),
        NonZeroU8::new(scale).unwrap(),
    )
}

/// A generator whose height and biome are functions of the block position.
pub struct Shaped<H, B> {
    pub height: H,
    pub biome: B,
}

impl<H, B> WorldGenerator for Shaped<H, B>
where
    H: Fn(i32, i32) -> i32 + Send + Sync,
    B: Fn(i32, i32) -> BiomeId + Send + Sync,
{
    fn generate_chunk(&self, pos: &ChunkPos) -> Chunk {
        ChunkBuilder::new(pos.x, pos.z).build()
    }

    fn surface_height_at(&self, x: i32, z: i32) -> i32 {
        (self.height)(x, z)
    }

    fn cell_biome(&self, x: i32, _y: i32, z: i32) -> BiomeId {
        (self.biome)(x, z)
    }
}

/// The same height and biome everywhere.
pub struct Flat {
    height: i32,
    biome: BiomeId,
}

impl Flat {
    pub fn new(height: i32, biome: BiomeId) -> Self {
        Self { height, biome }
    }
}

impl WorldGenerator for Flat {
    fn generate_chunk(&self, pos: &ChunkPos) -> Chunk {
        ChunkBuilder::new(pos.x, pos.z).build()
    }

    fn surface_height_at(&self, _x: i32, _z: i32) -> i32 {
        self.height
    }

    fn cell_biome(&self, _x: i32, _y: i32, _z: i32) -> BiomeId {
        self.biome
    }
}
