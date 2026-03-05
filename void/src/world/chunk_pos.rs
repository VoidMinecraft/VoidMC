/// A chunk column position in the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkPos {
    pub x: i32,
    pub z: i32,
}

impl ChunkPos {
    pub fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }

    /// Converts block coordinates to a chunk position.
    pub fn from_block(block_x: f64, block_z: f64) -> Self {
        Self {
            x: block_x.floor() as i32 >> 4,
            z: block_z.floor() as i32 >> 4,
        }
    }

    /// Returns the Chebyshev (chessboard) distance between two chunk positions.
    pub fn chebyshev_distance(&self, other: &ChunkPos) -> i32 {
        (self.x - other.x).abs().max((self.z - other.z).abs())
    }

    /// Returns all chunk positions within a given Chebyshev radius (inclusive),
    /// sorted by squared Euclidean distance from `self` (nearest first).
    pub fn chunks_in_radius(&self, radius: i32) -> Vec<ChunkPos> {
        let mut chunks = Vec::with_capacity(((radius * 2 + 1) * (radius * 2 + 1)) as usize);
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                chunks.push(ChunkPos::new(self.x + dx, self.z + dz));
            }
        }
        let cx = self.x;
        let cz = self.z;
        chunks.sort_by_key(|c| {
            let dx = c.x - cx;
            let dz = c.z - cz;
            dx * dx + dz * dz
        });
        chunks
    }
}
