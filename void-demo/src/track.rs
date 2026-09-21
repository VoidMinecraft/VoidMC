use std::{collections::HashMap, f64::consts::TAU, sync::Arc};

use bevy_ecs::prelude::Resource;
use voidmc::ChunkPos;
use voidmc_data::v26_1_2::blocks as b;
use voidmc_protocol::clientbound::ChunkBuilder;

use crate::terrain::{Alpine, mix};

pub const GATES: usize = 8;
pub const HALF_WIDTH: f64 = 7.0;
const SAMPLES_PER_KNOT: usize = 48;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Node {
    x: f64,
    y: f64,
    z: f64,
    distance: f64,
}

pub struct Projection {
    pub distance: f64,
    pub phase: f64,
    pub y: f64,
    pub normal: (f64, f64),
}

#[derive(Clone, Resource)]
pub struct Track {
    pub seed: u64,
    nodes: Arc<[Node]>,
    index: Arc<HashMap<ChunkPos, Vec<usize>>>,
    pub length: f64,
}

impl Track {
    pub fn new(seed: u64) -> Self {
        let mut state = seed;
        let mut random = || {
            state = state.wrapping_add(0x9e3779b97f4a7c15);
            (mix(state) >> 11) as f64 / (1u64 << 53) as f64
        };
        let count = 10 + (random() * 5.0) as usize;
        let rotation = random() * TAU;
        let stretch = 0.8 + random() * 0.4;
        let mut knots = Vec::new();
        for i in 0..count {
            let angle = (i as f64 + (random() - 0.5) * 0.35) * TAU / count as f64;
            let radius = 78.0 + random() * 48.0;
            let (x, z) = (radius * angle.cos() * stretch, radius * angle.sin());
            knots.push([
                x * rotation.cos() - z * rotation.sin(),
                80.0 + random() * 16.0,
                x * rotation.sin() + z * rotation.cos(),
            ]);
        }
        let mut nodes: Vec<Node> = Vec::new();
        let mut length = 0.0;
        for i in 0..=count * SAMPLES_PER_KNOT {
            let knot = i / SAMPLES_PER_KNOT;
            let t = (i % SAMPLES_PER_KNOT) as f64 / SAMPLES_PER_KNOT as f64;
            let p = std::array::from_fn::<_, 3, _>(|axis| {
                let [a, b, c, d] =
                    [count - 1, 0, 1, 2].map(|offset| knots[(knot + offset) % count][axis]);
                0.5 * ((2.0 * b)
                    + (-a + c) * t
                    + (2.0 * a - 5.0 * b + 4.0 * c - d) * t * t
                    + (-a + 3.0 * b - 3.0 * c + d) * t * t * t)
            });
            if let Some(previous) = nodes.last() {
                length += (p[0] - previous.x).hypot(p[2] - previous.z);
            }
            nodes.push(Node {
                x: p[0],
                y: p[1],
                z: p[2],
                distance: length,
            });
        }
        let mut index: HashMap<ChunkPos, Vec<usize>> = HashMap::new();
        for (i, segment) in nodes.windows(2).enumerate() {
            let [a, b] = [segment[0], segment[1]];
            let margin = HALF_WIDTH + 2.0;
            let min = ChunkPos::from_block(a.x.min(b.x) - margin, a.z.min(b.z) - margin);
            let max = ChunkPos::from_block(a.x.max(b.x) + margin, a.z.max(b.z) + margin);
            for z in min.z..=max.z {
                for x in min.x..=max.x {
                    index.entry(ChunkPos::new(x, z)).or_default().push(i);
                }
            }
        }
        Self {
            seed,
            nodes: nodes.into(),
            index: Arc::new(index),
            length,
        }
    }

    /// Reversed so `pop()` walks along the course, starting at the finish gantry.
    pub fn chunks(&self) -> Vec<ChunkPos> {
        let mut chunks: Vec<_> = self.index.keys().copied().collect();
        chunks.sort_by_key(|p| (self.index[p][0], p.x, p.z));
        chunks.reverse();
        chunks
    }

    fn segment_at(&self, phase: f64) -> (Node, Node, f64) {
        let distance = phase.rem_euclid(TAU) / TAU * self.length;
        let i = self
            .nodes
            .partition_point(|n| n.distance <= distance)
            .saturating_sub(1)
            .min(self.nodes.len() - 2);
        let (a, b) = (self.nodes[i], self.nodes[i + 1]);
        (a, b, (distance - a.distance) / (b.distance - a.distance))
    }

    pub fn point(&self, phase: f64, offset: f64) -> (f64, f64, f64) {
        let (a, b, t) = self.segment_at(phase);
        let len = (b.x - a.x).hypot(b.z - a.z);
        (
            a.x + (b.x - a.x) * t + (b.z - a.z) / len * offset,
            (a.y + (b.y - a.y) * t).floor() + 1.0,
            a.z + (b.z - a.z) * t - (b.x - a.x) / len * offset,
        )
    }

    pub fn heading(&self, phase: f64) -> f64 {
        let (a, b, _) = self.segment_at(phase);
        (a.x - b.x).atan2(b.z - a.z)
    }

    pub fn project(&self, x: f64, z: f64) -> Projection {
        let mut best = Projection {
            distance: f64::INFINITY,
            phase: 0.0,
            y: 0.0,
            normal: (1.0, 0.0),
        };
        if let Some(segments) = self.index.get(&ChunkPos::from_block(x, z)) {
            for &i in segments {
                let (a, b) = (self.nodes[i], self.nodes[i + 1]);
                let (dx, dz) = (b.x - a.x, b.z - a.z);
                let t = (((x - a.x) * dx + (z - a.z) * dz) / (dx * dx + dz * dz)).clamp(0.0, 1.0);
                let distance = (x - a.x - dx * t).hypot(z - a.z - dz * t);
                if distance < best.distance {
                    let length = dx.hypot(dz);
                    best = Projection {
                        distance,
                        phase: (a.distance + length * t) / self.length * TAU,
                        y: (a.y + (b.y - a.y) * t).floor() + 1.0,
                        normal: (dz / length, -dx / length),
                    };
                }
            }
        }
        best
    }

    pub fn road_distance(&self, x: f64, z: f64) -> f64 {
        self.project(x, z).distance
    }

    pub fn overlay(&self, pos: &ChunkPos, terrain: &Alpine, builder: &mut ChunkBuilder) {
        for z in 0..16u8 {
            for x in 0..16u8 {
                let (wx, wz) = (pos.x * 16 + i32::from(x), pos.z * 16 + i32::from(z));
                let p = self.project(f64::from(wx) + 0.5, f64::from(wz) + 0.5);
                if p.distance > HALF_WIDTH + 1.0 {
                    continue;
                }
                let y = p.y as i32 - 1;
                let h = terrain.height(wx, wz);
                let along = p.phase / TAU * self.length;
                let gate = p.phase / TAU * GATES as f64;
                let stripe = (gate - gate.round()).abs() * self.length / (GATES as f64) < 1.2;
                let edge = p.distance > HALF_WIDTH;
                let curb = p.distance > HALF_WIDTH - 0.9;
                let accent = if (gate.floor() as usize).is_multiple_of(2) {
                    b::CYAN_CONCRETE
                } else {
                    b::ORANGE_CONCRETE
                };
                let deck = if stripe && !(0.1..7.9).contains(&gate) {
                    if (wx + wz).rem_euclid(2) == 0 {
                        b::WHITE_CONCRETE
                    } else {
                        b::BLACK_CONCRETE
                    }
                } else if curb {
                    if (along as i32 / 3).rem_euclid(2) == 0 {
                        accent
                    } else {
                        b::WHITE_CONCRETE
                    }
                } else if stripe {
                    b::GOLD_BLOCK
                } else if p.distance < 0.25 && along.rem_euclid(8.0) < 4.0 {
                    b::WHITE_CONCRETE
                } else if terrain.hash(wx, wz).is_multiple_of(5) {
                    b::POLISHED_ANDESITE
                } else {
                    b::GRAY_CONCRETE
                };
                builder.set_block(x, y - 2, z, b::DEEPSLATE_BRICKS);
                builder.set_block(x, y - 1, z, b::STONE_BRICKS);
                builder.set_block(x, y, z, deck);
                for clear in (y + 1)..=h.saturating_add(16).max(y + 10) {
                    builder.set_block(x, clear, z, b::AIR);
                }
                if edge {
                    builder.set_block(x, y + 1, z, b::CYAN_STAINED_GLASS);
                    if along.rem_euclid(16.0) < 1.5 {
                        builder.set_block(x, y + 1, z, b::SEA_LANTERN);
                        for support in h..y - 2 {
                            builder.set_block(x, support, z, b::DEEPSLATE_BRICKS);
                        }
                    }
                }
                if stripe {
                    if edge {
                        for pillar in y + 1..y + 7 {
                            builder.set_block(x, pillar, z, accent);
                        }
                    }
                    builder.set_block(
                        x,
                        y + 7,
                        z,
                        if p.distance < 3.0 {
                            b::SEA_LANTERN
                        } else {
                            b::QUARTZ_BLOCK
                        },
                    );
                    builder.set_block(x, y + 8, z, accent);
                }
                let station = (gate - gate.floor() - 0.45).abs() * self.length / GATES as f64;
                if station < 1.4 && (p.distance < 1.1 || (p.distance - 3.5).abs() < 1.1) {
                    builder.set_block(
                        x,
                        y,
                        z,
                        if station < 0.6 {
                            b::SEA_LANTERN
                        } else {
                            b::AMETHYST_BLOCK
                        },
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_splines_are_closed_driveable_and_spatially_indexed() {
        for seed in (0..64).chain([u64::MAX]) {
            let track = Track::new(seed);
            assert!((450.0..1100.0).contains(&track.length));
            assert_eq!(
                track.nodes.first().unwrap().x,
                track.nodes.last().unwrap().x
            );
            for (i, n) in track.nodes.iter().enumerate().take(track.nodes.len() - 1) {
                let p = track.project(n.x, n.z);
                assert!(p.distance < 1e-8);
                assert!((p.phase - n.distance / track.length * TAU).abs() < 1e-8);
                assert!((n.x.hypot(n.z)) > 45.0);
                let b = track.nodes[i + 1];
                assert!((b.y - n.y).abs() < 0.6);
                assert!((b.x - n.x).hypot(b.z - n.z) < 2.5);
            }
        }
    }

    #[test]
    fn all_seed_bits_change_shape_and_elevation_reproducibly() {
        let a = Track::new(42);
        assert_eq!(&*a.nodes, &*Track::new(42).nodes);
        for seed in [43, 10042, 42 + (1 << 48)] {
            let b = Track::new(seed);
            assert!((a.length - b.length).abs() > 1.0);
            assert_ne!(a.point(0.0, 0.0), b.point(0.0, 0.0));
            assert_ne!(a.point(1.0, 0.0).1, b.point(1.0, 0.0).1);
        }
    }

    #[test]
    fn chunk_order_follows_the_course_from_the_finish_gantry() {
        let track = Track::new(42);
        let mut chunks = track.chunks();
        assert!(chunks.len() > 20);
        let first = chunks.pop().unwrap();
        let (x, _, z) = track.point(0.0, 0.0);
        assert!(first.chebyshev_distance(&ChunkPos::from_block(x, z)) <= 1);
        let mut seen = std::collections::HashSet::from([first]);
        assert!(chunks.iter().all(|c| seen.insert(*c)));
    }
}
