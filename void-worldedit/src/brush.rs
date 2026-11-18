use std::fmt;

use crate::block::BlockState;
use crate::extent::Extent;
use crate::mask::Mask;
use crate::math::{BlockPos, Vec3};
use crate::operation::{BlockList, Fill, Operation};
use crate::pattern::Pattern;
use crate::region::{Cylinder, Ellipsoid, Region};

pub const MAX_BRUSH_RADIUS: f64 = 32.0;

#[derive(Clone, Debug, PartialEq)]
pub enum BrushShape {
    Sphere {
        pattern: Pattern,
        radius: f64,
        hollow: bool,
    },
    Cylinder {
        pattern: Pattern,
        radius: f64,
        height: i32,
        hollow: bool,
    },
    /// Blurs the terrain heightmap under the target, `iterations` times.
    Smooth { radius: f64, iterations: u32 },
    /// Drops every block of the sphere down onto whatever is below it.
    Gravity { radius: f64 },
}

/// A brush bound to a tool: what it does where the player looks, the mask of
/// blocks it may change, and how far it reaches.
#[derive(Clone, Debug, PartialEq)]
pub struct Brush {
    pub shape: BrushShape,
    pub mask: Mask,
    pub range: u32,
}

impl Brush {
    pub fn new(shape: BrushShape) -> Self {
        Self {
            shape,
            mask: Mask::Any,
            range: 128,
        }
    }

    pub fn radius(&self) -> f64 {
        match &self.shape {
            BrushShape::Sphere { radius, .. }
            | BrushShape::Cylinder { radius, .. }
            | BrushShape::Smooth { radius, .. }
            | BrushShape::Gravity { radius } => *radius,
        }
    }

    /// The edit this brush makes when it hits `target`. Smooth and gravity
    /// read the terrain now, so the result reflects the world at click time.
    pub fn operation(
        &self,
        extent: &dyn Extent,
        target: BlockPos,
        seed: u64,
    ) -> Box<dyn Operation> {
        match &self.shape {
            BrushShape::Sphere {
                pattern,
                radius,
                hollow,
            } => {
                let mut sphere = Ellipsoid::sphere(target, *radius);
                sphere.hollow = *hollow;
                Box::new(
                    Fill::new(sphere, pattern.clone().with_seed(seed)).masked(self.mask.clone()),
                )
            }
            BrushShape::Cylinder {
                pattern,
                radius,
                height,
                hollow,
            } => {
                let mut cylinder = Cylinder::new(target, *radius, *height);
                cylinder.hollow = *hollow;
                Box::new(
                    Fill::new(cylinder, pattern.clone().with_seed(seed)).masked(self.mask.clone()),
                )
            }
            BrushShape::Smooth { radius, iterations } => {
                Box::new(smooth(extent, target, *radius, *iterations, &self.mask))
            }
            BrushShape::Gravity { radius } => {
                Box::new(gravity(extent, target, *radius, &self.mask))
            }
        }
    }
}

impl fmt::Display for BrushShape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BrushShape::Sphere { radius, hollow, .. } => {
                write!(
                    f,
                    "{}sphere, radius {radius}",
                    if *hollow { "hollow " } else { "" }
                )
            }
            BrushShape::Cylinder {
                radius,
                height,
                hollow,
                ..
            } => write!(
                f,
                "{}cylinder, radius {radius}, height {height}",
                if *hollow { "hollow " } else { "" }
            ),
            BrushShape::Smooth { radius, iterations } => {
                write!(f, "smooth, radius {radius}, {iterations} iterations")
            }
            BrushShape::Gravity { radius } => write!(f, "gravity, radius {radius}"),
        }
    }
}

/// The first non-air block along a ray, stepping voxel by voxel (Amanatides
/// and Woo), or `None` past `range` blocks or at unloaded terrain.
pub fn raycast(extent: &dyn Extent, origin: Vec3, direction: Vec3, range: f64) -> Option<BlockPos> {
    let length = direction.length();
    if length == 0.0 {
        return None;
    }
    let dir = direction * (1.0 / length);
    let mut cell = origin.block();
    let step = |d: f64| {
        if d > 0.0 {
            1
        } else if d < 0.0 {
            -1
        } else {
            0
        }
    };
    let (sx, sy, sz) = (step(dir.x), step(dir.y), step(dir.z));
    let boundary = |o: f64, c: i32, s: i32| {
        if s > 0 {
            f64::from(c + 1) - o
        } else {
            o - f64::from(c)
        }
    };
    let delta = |d: f64| {
        if d == 0.0 {
            f64::INFINITY
        } else {
            1.0 / d.abs()
        }
    };
    let (dx, dy, dz) = (delta(dir.x), delta(dir.y), delta(dir.z));
    let mut tx = if sx == 0 {
        f64::INFINITY
    } else {
        boundary(origin.x, cell.x, sx) * dx
    };
    let mut ty = if sy == 0 {
        f64::INFINITY
    } else {
        boundary(origin.y, cell.y, sy) * dy
    };
    let mut tz = if sz == 0 {
        f64::INFINITY
    } else {
        boundary(origin.z, cell.z, sz) * dz
    };
    loop {
        if !extent.block(cell)?.is_air() {
            return Some(cell);
        }
        let travelled = tx.min(ty).min(tz);
        if travelled > range {
            return None;
        }
        if tx <= ty && tx <= tz {
            cell.x += sx;
            tx += dx;
        } else if ty <= tz {
            cell.y += sy;
            ty += dy;
        } else {
            cell.z += sz;
            tz += dz;
        }
    }
}

fn smooth(
    extent: &dyn Extent,
    target: BlockPos,
    radius: f64,
    iterations: u32,
    mask: &Mask,
) -> BlockList {
    let r = radius.clamp(0.0, MAX_BRUSH_RADIUS).floor() as i32;
    let (min_y, max_y) = extent.height();
    let low = (target.y - r).max(min_y);
    let high = (target.y + r).min(max_y);
    let width = (2 * r + 1) as usize;
    let column = |x: i32, z: i32| -> Option<(i32, BlockState)> {
        (low..=high).rev().find_map(|y| {
            let state = extent.block(BlockPos::new(x, y, z))?;
            (!state.is_air()).then_some((y, state))
        })
    };
    let mut tops = vec![None; width * width];
    for dz in -r..=r {
        for dx in -r..=r {
            tops[(dz + r) as usize * width + (dx + r) as usize] =
                column(target.x + dx, target.z + dz);
        }
    }
    let original: Vec<f64> = tops
        .iter()
        .map(|top| top.map_or(f64::from(low - 1), |(y, _)| f64::from(y)))
        .collect();
    let mut heights = original.clone();
    for _ in 0..iterations {
        let previous = heights.clone();
        for z in 0..width {
            for x in 0..width {
                let mut sum = 0.0;
                let mut weight = 0.0;
                for (oz, ox, w) in GAUSSIAN {
                    let (nx, nz) = (x as i32 + ox, z as i32 + oz);
                    if (0..width as i32).contains(&nx) && (0..width as i32).contains(&nz) {
                        sum += previous[nz as usize * width + nx as usize] * w;
                        weight += w;
                    }
                }
                heights[z * width + x] = sum / weight;
            }
        }
    }
    let mut changes = BlockList::new();
    for dz in -r..=r {
        for dx in -r..=r {
            if dx * dx + dz * dz > r * r + r {
                continue;
            }
            let index = (dz + r) as usize * width + (dx + r) as usize;
            let Some((old_top, surface)) = tops[index] else {
                continue;
            };
            let new_top = (heights[index].round() as i32).clamp(low - 1, high);
            let (x, z) = (target.x + dx, target.z + dz);
            if new_top > old_top {
                for y in old_top + 1..=new_top {
                    let pos = BlockPos::new(x, y, z);
                    if extent.block(pos).is_some_and(|s| mask.test(s)) {
                        changes.set(pos, surface);
                    }
                }
            } else if new_top < old_top {
                for y in new_top + 1..=old_top {
                    let pos = BlockPos::new(x, y, z);
                    if extent.block(pos).is_some_and(|s| mask.test(s)) {
                        changes.set(pos, BlockState::AIR);
                    }
                }
                if new_top >= low {
                    let pos = BlockPos::new(x, new_top, z);
                    if extent.block(pos).is_some_and(|s| mask.test(s)) {
                        changes.set(pos, surface);
                    }
                }
            }
        }
    }
    changes
}

const GAUSSIAN: [(i32, i32, f64); 9] = [
    (-1, -1, 1.0),
    (-1, 0, 2.0),
    (-1, 1, 1.0),
    (0, -1, 2.0),
    (0, 0, 4.0),
    (0, 1, 2.0),
    (1, -1, 1.0),
    (1, 0, 2.0),
    (1, 1, 1.0),
];

fn gravity(extent: &dyn Extent, target: BlockPos, radius: f64, mask: &Mask) -> BlockList {
    let sphere = Ellipsoid::sphere(target, radius.clamp(0.0, MAX_BRUSH_RADIUS));
    let bounds = sphere.bounds();
    let (min_y, max_y) = extent.height();
    let mut changes = BlockList::new();
    for z in bounds.min.z..=bounds.max.z {
        for x in bounds.min.x..=bounds.max.x {
            let ys: Vec<i32> = (bounds.min.y.max(min_y)..=bounds.max.y.min(max_y))
                .filter(|y| sphere.contains(BlockPos::new(x, *y, z)))
                .collect();
            let Some(before) = ys
                .iter()
                .map(|y| extent.block(BlockPos::new(x, *y, z)))
                .collect::<Option<Vec<BlockState>>>()
            else {
                continue;
            };
            let mut after = vec![BlockState::AIR; before.len()];
            let mut floor = 0;
            for (index, state) in before.iter().enumerate() {
                if state.is_air() {
                    continue;
                }
                if mask.test(*state) {
                    after[floor] = *state;
                    floor += 1;
                } else {
                    after[index] = *state;
                    floor = index + 1;
                }
            }
            for ((y, old), new) in ys.iter().zip(&before).zip(&after) {
                if old != new {
                    changes.set(BlockPos::new(x, *y, z), *new);
                }
            }
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extent::MemoryExtent;
    use crate::job::EditJob;
    use crate::region::Cuboid;

    fn state(input: &str) -> BlockState {
        BlockState::parse(input).unwrap()
    }

    #[test]
    fn raycast_hits_the_first_solid_block() {
        let mut extent = MemoryExtent::default();
        extent.set(BlockPos::new(5, 64, 0), state("stone"));
        extent.set(BlockPos::new(8, 64, 0), state("stone"));
        let hit = raycast(
            &extent,
            Vec3::new(0.5, 64.5, 0.5),
            Vec3::new(1.0, 0.0, 0.0),
            50.0,
        );
        assert_eq!(hit, Some(BlockPos::new(5, 64, 0)));
        assert_eq!(
            raycast(
                &extent,
                Vec3::new(0.5, 64.5, 0.5),
                Vec3::new(-1.0, 0.0, 0.0),
                50.0
            ),
            None
        );
        assert_eq!(
            raycast(
                &extent,
                Vec3::new(0.5, 64.5, 0.5),
                Vec3::new(1.0, 0.0, 0.0),
                3.0
            ),
            None
        );
    }

    #[test]
    fn raycast_follows_diagonals() {
        let mut extent = MemoryExtent::default();
        extent.set(BlockPos::new(3, 61, 3), state("stone"));
        let hit = raycast(
            &extent,
            Vec3::new(0.5, 64.5, 0.5),
            Vec3::new(1.0, -1.0, 1.0),
            20.0,
        );
        assert_eq!(hit, Some(BlockPos::new(3, 61, 3)));
    }

    #[test]
    fn sphere_brush_respects_its_mask() {
        let mut extent = MemoryExtent::default();
        for pos in Cuboid::new(BlockPos::new(-5, 60, -5), BlockPos::new(5, 64, 5)).positions() {
            extent.set(pos, state("dirt"));
        }
        let mut brush = Brush::new(BrushShape::Sphere {
            pattern: Pattern::Block(state("stone")),
            radius: 3.0,
            hollow: false,
        });
        brush.mask = Mask::Existing;
        let target = BlockPos::new(0, 64, 0);
        let op = brush.operation(&extent, target, 0);
        EditJob::from_arc(op.into(), (-64, 319)).run(&mut extent);
        assert_eq!(extent.block(target), Some(state("stone")));
        assert_eq!(extent.block(BlockPos::new(0, 66, 0)), Some(BlockState::AIR));
    }

    #[test]
    fn smooth_flattens_a_spike() {
        let mut extent = MemoryExtent::default();
        for pos in Cuboid::new(BlockPos::new(-6, 60, -6), BlockPos::new(6, 63, 6)).positions() {
            extent.set(pos, state("grass_block"));
        }
        for y in 64..70 {
            extent.set(BlockPos::new(0, y, 0), state("grass_block"));
        }
        let brush = Brush::new(BrushShape::Smooth {
            radius: 5.0,
            iterations: 4,
        });
        let op = brush.operation(&extent, BlockPos::new(0, 64, 0), 0);
        EditJob::from_arc(op.into(), (-64, 319)).run(&mut extent);
        assert_eq!(extent.block(BlockPos::new(0, 69, 0)), Some(BlockState::AIR));
        assert_eq!(
            extent.block(BlockPos::new(0, 63, 0)),
            Some(state("grass_block"))
        );
    }

    #[test]
    fn gravity_drops_floating_blocks_onto_the_ground() {
        let mut extent = MemoryExtent::default();
        for x in -4..=4 {
            for z in -4..=4 {
                extent.set(BlockPos::new(x, 60, z), state("stone"));
            }
        }
        extent.set(BlockPos::new(0, 64, 0), state("sand"));
        extent.set(BlockPos::new(0, 66, 0), state("gravel"));
        let brush = Brush::new(BrushShape::Gravity { radius: 4.0 });
        let op = brush.operation(&extent, BlockPos::new(0, 64, 0), 0);
        EditJob::from_arc(op.into(), (-64, 319)).run(&mut extent);
        assert_eq!(extent.block(BlockPos::new(0, 60, 0)), Some(state("stone")));
        assert_eq!(extent.block(BlockPos::new(3, 60, 0)), Some(state("stone")));
        assert_eq!(extent.block(BlockPos::new(0, 61, 0)), Some(state("sand")));
        assert_eq!(extent.block(BlockPos::new(0, 62, 0)), Some(state("gravel")));
        assert_eq!(extent.block(BlockPos::new(0, 64, 0)), Some(BlockState::AIR));
        assert_eq!(extent.block(BlockPos::new(0, 66, 0)), Some(BlockState::AIR));
    }
}
