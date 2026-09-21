use bevy_ecs::prelude::Resource;
use voidmc::{BiomeId, ChunkPos, WorldGenerator};
use voidmc_data::v26_1_2::blocks as b;
use voidmc_protocol::clientbound::{Chunk, ChunkBuilder};

pub const WATER_LEVEL: i32 = 62;

#[derive(Clone, Resource)]
pub struct Alpine {
    pub seed: u64,
}

pub fn mix(mut n: u64) -> u64 {
    n = (n ^ (n >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    n = (n ^ (n >> 27)).wrapping_mul(0x94d049bb133111eb);
    n ^ (n >> 31)
}

pub fn biome() -> BiomeId {
    BiomeId::named("taiga").unwrap_or_else(BiomeId::plains)
}

impl Alpine {
    pub fn hash(&self, x: i32, z: i32) -> u64 {
        mix(self.seed
            ^ (x as u64).wrapping_mul(0x9e3779b97f4a7c15)
            ^ (z as u64).wrapping_mul(0xbf58476d1ce4e5b9))
    }

    fn noise(&self, x: f64, z: f64, scale: f64) -> f64 {
        let (x, z) = (x / scale, z / scale);
        let (ix, iz) = (x.floor() as i32, z.floor() as i32);
        let smooth = |t: f64| t * t * (3.0 - 2.0 * t);
        let (tx, tz) = (smooth(x - x.floor()), smooth(z - z.floor()));
        let value = |dx, dz| (self.hash(ix + dx, iz + dz) >> 11) as f64 / (1u64 << 53) as f64;
        let a = value(0, 0) * (1.0 - tx) + value(1, 0) * tx;
        let b = value(0, 1) * (1.0 - tx) + value(1, 1) * tx;
        a * (1.0 - tz) + b * tz
    }

    pub fn height(&self, x: i32, z: i32) -> i32 {
        let (x, z) = (f64::from(x), f64::from(z));
        let r = x.hypot(z);
        let mountains = ((r - 105.0) / 95.0).clamp(0.0, 1.0);
        let ridge = 1.0 - (self.noise(x, z, 85.0) * 2.0 - 1.0).abs();
        let island = 39.0 * (-r * r / 700.0).exp();
        (51.0
            + 21.0 * self.noise(x, z, 45.0)
            + 5.0 * self.noise(x, z, 13.0)
            + island
            + mountains * (28.0 + 98.0 * ridge.powi(2))) as i32
    }

    pub fn builder(&self, pos: &ChunkPos) -> ChunkBuilder {
        let mut builder = ChunkBuilder::new(pos.x, pos.z)
            .biome(biome().0)
            .with_heightmap_layered(
                |x, z| self.height(x, z),
                &[(1, b::GRASS_BLOCK), (4, b::DIRT), (i32::MAX, b::STONE)],
            )
            .add_water(WATER_LEVEL);
        for z in 0..16u8 {
            for x in 0..16u8 {
                let (wx, wz) = (pos.x * 16 + i32::from(x), pos.z * 16 + i32::from(z));
                let h = self.height(wx, wz);
                let hash = self.hash(wx, wz);
                let slope = (h - self.height(wx + 1, wz))
                    .abs()
                    .max((h - self.height(wx, wz + 1)).abs());
                if h > 134 + (hash % 7) as i32 {
                    builder.set_block(x, h - 1, z, b::SNOW_BLOCK);
                } else if h > 108 || slope > 2 {
                    builder.set_block(
                        x,
                        h - 1,
                        z,
                        if hash.is_multiple_of(4) {
                            b::ANDESITE
                        } else {
                            b::STONE
                        },
                    );
                } else if h <= 64 {
                    builder.set_block(
                        x,
                        h - 1,
                        z,
                        if hash.is_multiple_of(5) {
                            b::GRAVEL
                        } else {
                            b::SAND
                        },
                    );
                } else if hash.is_multiple_of(31) {
                    builder.set_block(
                        x,
                        h,
                        z,
                        if hash.is_multiple_of(3) {
                            b::OXEYE_DAISY
                        } else {
                            b::FERN
                        },
                    );
                }
            }
        }
        self.decorate(pos, &mut builder);
        builder
    }

    fn decorate(&self, pos: &ChunkPos, builder: &mut ChunkBuilder) {
        let mut put = |x: i32, y: i32, z: i32, block| {
            if x.div_euclid(16) == pos.x && z.div_euclid(16) == pos.z {
                builder.set_block(x.rem_euclid(16) as u8, y, z.rem_euclid(16) as u8, block);
            }
        };
        for z in pos.z * 16 - 4..pos.z * 16 + 20 {
            for x in pos.x * 16 - 4..pos.x * 16 + 20 {
                if x.rem_euclid(7) != 0 || z.rem_euclid(7) != 0 || x.abs().max(z.abs()) < 18 {
                    continue;
                }
                let h = self.height(x, z);
                let hash = self.hash(x, z);
                if !(65..116).contains(&h) {
                    continue;
                }
                if hash.is_multiple_of(5) {
                    for dx in -2i32..=2 {
                        for dz in -2i32..=2 {
                            let top = 3 - dx.abs() - dz.abs();
                            for dy in -2..top {
                                put(
                                    x + dx,
                                    h + dy,
                                    z + dz,
                                    if dy == top - 1 {
                                        b::MOSSY_COBBLESTONE
                                    } else {
                                        b::COBBLESTONE
                                    },
                                );
                            }
                        }
                    }
                } else if hash.is_multiple_of(3) {
                    let top = h + 8 + (hash % 5) as i32;
                    for y in h..top {
                        put(x, y, z, b::SPRUCE_LOG);
                    }
                    for y in h + 3..=top {
                        let width = ((top - y) / 3 + 1).min(3);
                        for dz in -width..=width {
                            for dx in -width..=width {
                                if dx.abs() + dz.abs() <= width + 1
                                    && (dx != 0 || dz != 0 || y == top)
                                {
                                    put(x + dx, y, z + dz, b::SPRUCE_LEAVES);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

impl WorldGenerator for Alpine {
    fn generate_chunk(&self, pos: &ChunkPos) -> Chunk {
        self.builder(pos).build()
    }

    fn surface_height_at(&self, x: i32, z: i32) -> i32 {
        self.height(x, z).max(WATER_LEVEL)
    }

    fn cell_biome(&self, _x: i32, _y: i32, _z: i32) -> BiomeId {
        biome()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(chunk: &Chunk) -> Vec<Vec<u8>> {
        chunk.sections.iter().map(|s| s.encode_to_bytes()).collect()
    }

    #[test]
    fn landscape_is_repeatable_and_uses_all_seed_bits() {
        let map = Alpine { seed: 42 };
        let pos = ChunkPos::new(-1, 0);
        assert_eq!(
            encode(&map.generate_chunk(&pos)),
            encode(&Alpine { seed: 42 }.generate_chunk(&pos))
        );
        let shifted = Alpine {
            seed: 42 + (1 << 48),
        };
        assert_ne!(map.hash(128, -64), shifted.hash(128, -64));
        assert_ne!(
            encode(&map.generate_chunk(&pos)),
            encode(&shifted.generate_chunk(&pos))
        );
    }

    #[test]
    fn surface_never_drops_below_water_and_peaks_carry_snow() {
        let map = Alpine { seed: 42 };
        let mut snow = false;
        for z in -256..256 {
            for x in (-256..256).step_by(8) {
                assert!(map.surface_height_at(x, z) >= WATER_LEVEL);
                snow |= map.height(x, z) > 141;
            }
        }
        assert!(snow);
        assert_eq!(biome(), BiomeId::named("minecraft:taiga").unwrap());
    }
}
