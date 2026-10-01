use std::fmt::Debug;

use crate::math::{BlockPos, Direction, SectionPos};

/// A set of block positions. Fills walk a region row by row through
/// [`for_each_span`](Region::for_each_span), so shapes only have to answer
/// "which x runs of this (y, z) row are inside".
pub trait Region: Send + Sync + Debug {
    fn bounds(&self) -> Cuboid;

    fn contains(&self, pos: BlockPos) -> bool;

    /// Whether every block of `cuboid` is inside; `false` is always safe.
    fn covers(&self, _cuboid: &Cuboid) -> bool {
        false
    }

    /// Calls `f(min_x, max_x)` for every inclusive run of the row at `(y, z)`
    /// inside the region, clipped to `x_min..=x_max`.
    fn for_each_span(&self, y: i32, z: i32, x_min: i32, x_max: i32, f: &mut dyn FnMut(i32, i32)) {
        default_spans(self, y, z, x_min, x_max, f);
    }

    fn volume(&self) -> u64 {
        let bounds = self.bounds();
        let mut volume = 0;
        for y in bounds.min.y..=bounds.max.y {
            for z in bounds.min.z..=bounds.max.z {
                self.for_each_span(y, z, bounds.min.x, bounds.max.x, &mut |a, b| {
                    volume += (b - a + 1) as u64;
                });
            }
        }
        volume
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Cuboid {
    pub min: BlockPos,
    pub max: BlockPos,
}

impl Cuboid {
    pub fn new(a: BlockPos, b: BlockPos) -> Self {
        Self {
            min: a.min(b),
            max: a.max(b),
        }
    }

    pub fn size(&self) -> BlockPos {
        self.max - self.min + BlockPos::new(1, 1, 1)
    }

    /// Saturates at `u64::MAX`, so a limit check fails closed.
    pub fn cuboid_volume(&self) -> u64 {
        let side = |lo: i32, hi: i32| (i64::from(hi) - i64::from(lo) + 1).max(0) as u64;
        side(self.min.x, self.max.x)
            .saturating_mul(side(self.min.y, self.max.y))
            .saturating_mul(side(self.min.z, self.max.z))
    }

    pub fn intersection(&self, other: &Cuboid) -> Option<Cuboid> {
        let min = self.min.max(other.min);
        let max = self.max.min(other.max);
        (min.x <= max.x && min.y <= max.y && min.z <= max.z).then_some(Cuboid { min, max })
    }

    pub fn contains_cuboid(&self, other: &Cuboid) -> bool {
        self.intersection(other) == Some(*other)
    }

    pub fn shifted(&self, offset: BlockPos) -> Self {
        Self {
            min: self.min + offset,
            max: self.max + offset,
        }
    }

    /// Pushes the face looking towards `direction` outwards by `amount`.
    pub fn expanded(&self, direction: Direction, amount: i32) -> Self {
        let mut cuboid = *self;
        let delta = direction.vector() * amount.max(0);
        if positive(delta) {
            cuboid.max += delta;
        } else {
            cuboid.min += delta;
        }
        cuboid
    }

    /// Like WorldEdit, moves the face opposite `direction` along it by
    /// `amount`, never past the other face: contracting up raises the floor.
    pub fn contracted(&self, direction: Direction, amount: i32) -> Self {
        let mut cuboid = *self;
        let delta = direction.vector() * amount.max(0);
        if positive(delta) {
            cuboid.min = (cuboid.min + delta).min(cuboid.max);
        } else {
            cuboid.max = (cuboid.max + delta).max(cuboid.min);
        }
        cuboid
    }

    pub fn sections(&self) -> impl Iterator<Item = SectionPos> + use<> {
        let (min, max) = (self.min.section(), self.max.section());
        (min.x..=max.x).flat_map(move |x| {
            (min.z..=max.z)
                .flat_map(move |z| (min.y..=max.y).map(move |y| SectionPos::new(x, y, z)))
        })
    }

    pub fn of_section(section: SectionPos) -> Self {
        let min = section.min_block();
        Self {
            min,
            max: min.offset(15, 15, 15),
        }
    }

    pub fn positions(&self) -> impl Iterator<Item = BlockPos> + use<> {
        let (min, max) = (self.min, self.max);
        (min.y..=max.y).flat_map(move |y| {
            (min.z..=max.z).flat_map(move |z| (min.x..=max.x).map(move |x| BlockPos::new(x, y, z)))
        })
    }
}

impl Region for Cuboid {
    fn bounds(&self) -> Cuboid {
        *self
    }

    fn contains(&self, pos: BlockPos) -> bool {
        (self.min.x..=self.max.x).contains(&pos.x)
            && (self.min.y..=self.max.y).contains(&pos.y)
            && (self.min.z..=self.max.z).contains(&pos.z)
    }

    fn for_each_span(&self, y: i32, z: i32, x_min: i32, x_max: i32, f: &mut dyn FnMut(i32, i32)) {
        if (self.min.y..=self.max.y).contains(&y) && (self.min.z..=self.max.z).contains(&z) {
            let (from, to) = (x_min.max(self.min.x), x_max.min(self.max.x));
            if from <= to {
                f(from, to);
            }
        }
    }

    fn covers(&self, cuboid: &Cuboid) -> bool {
        self.contains_cuboid(cuboid)
    }

    fn volume(&self) -> u64 {
        self.cuboid_volume()
    }
}

fn positive(delta: BlockPos) -> bool {
    delta.x > 0 || delta.y > 0 || delta.z > 0
}

fn corners(cuboid: &Cuboid) -> impl Iterator<Item = BlockPos> + '_ {
    (0..8).map(move |i| {
        BlockPos::new(
            if i & 1 == 0 {
                cuboid.min.x
            } else {
                cuboid.max.x
            },
            if i & 2 == 0 {
                cuboid.min.y
            } else {
                cuboid.max.y
            },
            if i & 4 == 0 {
                cuboid.min.z
            } else {
                cuboid.max.z
            },
        )
    })
}

/// An axis-aligned ellipsoid; a sphere has the same radius on every axis.
/// Like WorldEdit, a block is inside when its offset from the center,
/// scaled by `radius + 0.5`, lies in the unit ball.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ellipsoid {
    pub center: BlockPos,
    pub radius: [f64; 3],
    pub hollow: bool,
}

impl Ellipsoid {
    pub fn sphere(center: BlockPos, radius: f64) -> Self {
        Self {
            center,
            radius: [radius; 3],
            hollow: false,
        }
    }

    pub fn hollow(mut self) -> Self {
        self.hollow = true;
        self
    }

    fn inverse(&self, shrink: f64) -> Option<[f64; 3]> {
        let r = self.radius.map(|r| r + 0.5 - shrink);
        r.iter().all(|r| *r > 0.0).then(|| r.map(|r| 1.0 / (r * r)))
    }

    fn half_width(inv: [f64; 3], dy: f64, dz: f64) -> Option<f64> {
        let rest = 1.0 - dy * dy * inv[1] - dz * dz * inv[2];
        (rest >= 0.0).then(|| (rest / inv[0]).sqrt())
    }
}

impl Region for Ellipsoid {
    fn bounds(&self) -> Cuboid {
        let r = self.radius.map(|r| (r + 0.5).floor() as i32);
        Cuboid {
            min: self.center.offset(-r[0], -r[1], -r[2]),
            max: self.center.offset(r[0], r[1], r[2]),
        }
    }

    fn contains(&self, pos: BlockPos) -> bool {
        let Some(outer) = self.inverse(0.0) else {
            return false;
        };
        let d = pos - self.center;
        let (dx, dy, dz) = (f64::from(d.x), f64::from(d.y), f64::from(d.z));
        let inside = |inv: [f64; 3]| dx * dx * inv[0] + dy * dy * inv[1] + dz * dz * inv[2] <= 1.0;
        inside(outer) && !(self.hollow && self.inverse(1.0).is_some_and(inside))
    }

    fn covers(&self, cuboid: &Cuboid) -> bool {
        !self.hollow && corners(cuboid).all(|corner| self.contains(corner))
    }

    fn for_each_span(&self, y: i32, z: i32, x_min: i32, x_max: i32, f: &mut dyn FnMut(i32, i32)) {
        if self.hollow {
            return default_spans(self, y, z, x_min, x_max, f);
        }
        let Some(inv) = self.inverse(0.0) else {
            return;
        };
        let (dy, dz) = (f64::from(y - self.center.y), f64::from(z - self.center.z));
        if let Some(half) = Self::half_width(inv, dy, dz) {
            let half = half.floor() as i32;
            let (from, to) = (
                x_min.max(self.center.x - half),
                x_max.min(self.center.x + half),
            );
            if from <= to {
                f(from, to);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cylinder {
    pub center: BlockPos,
    pub radius: [f64; 2],
    pub min_y: i32,
    pub max_y: i32,
    pub hollow: bool,
}

impl Cylinder {
    pub fn new(base: BlockPos, radius: f64, height: i32) -> Self {
        let top = base.y + height.max(1) - 1;
        Self {
            center: base,
            radius: [radius; 2],
            min_y: base.y.min(top),
            max_y: base.y.max(top),
            hollow: false,
        }
    }

    pub fn hollow(mut self) -> Self {
        self.hollow = true;
        self
    }

    fn inside(&self, dx: f64, dz: f64, shrink: f64) -> bool {
        let rx = self.radius[0] + 0.5 - shrink;
        let rz = self.radius[1] + 0.5 - shrink;
        rx > 0.0 && rz > 0.0 && dx * dx / (rx * rx) + dz * dz / (rz * rz) <= 1.0
    }
}

impl Region for Cylinder {
    fn bounds(&self) -> Cuboid {
        let (rx, rz) = (
            (self.radius[0] + 0.5).floor() as i32,
            (self.radius[1] + 0.5).floor() as i32,
        );
        Cuboid {
            min: BlockPos::new(self.center.x - rx, self.min_y, self.center.z - rz),
            max: BlockPos::new(self.center.x + rx, self.max_y, self.center.z + rz),
        }
    }

    fn contains(&self, pos: BlockPos) -> bool {
        if !(self.min_y..=self.max_y).contains(&pos.y) {
            return false;
        }
        let (dx, dz) = (
            f64::from(pos.x - self.center.x),
            f64::from(pos.z - self.center.z),
        );
        self.inside(dx, dz, 0.0) && !(self.hollow && self.inside(dx, dz, 1.0))
    }

    fn covers(&self, cuboid: &Cuboid) -> bool {
        !self.hollow && corners(cuboid).all(|corner| self.contains(corner))
    }

    fn for_each_span(&self, y: i32, z: i32, x_min: i32, x_max: i32, f: &mut dyn FnMut(i32, i32)) {
        if self.hollow {
            return default_spans(self, y, z, x_min, x_max, f);
        }
        if !(self.min_y..=self.max_y).contains(&y) {
            return;
        }
        let rx = self.radius[0] + 0.5;
        let rz = self.radius[1] + 0.5;
        let dz = f64::from(z - self.center.z);
        let rest = 1.0 - dz * dz / (rz * rz);
        if rest < 0.0 {
            return;
        }
        let half = (rx * rest.sqrt()).floor() as i32;
        let (from, to) = (
            x_min.max(self.center.x - half),
            x_max.min(self.center.x + half),
        );
        if from <= to {
            f(from, to);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Walls(pub Cuboid);

impl Region for Walls {
    fn bounds(&self) -> Cuboid {
        self.0
    }

    fn contains(&self, pos: BlockPos) -> bool {
        let c = &self.0;
        c.contains(pos)
            && (pos.x == c.min.x || pos.x == c.max.x || pos.z == c.min.z || pos.z == c.max.z)
    }

    fn for_each_span(&self, y: i32, z: i32, x_min: i32, x_max: i32, f: &mut dyn FnMut(i32, i32)) {
        shell_spans(
            &self.0,
            z == self.0.min.z || z == self.0.max.z,
            y,
            z,
            x_min,
            x_max,
            f,
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Faces(pub Cuboid);

impl Region for Faces {
    fn bounds(&self) -> Cuboid {
        self.0
    }

    fn contains(&self, pos: BlockPos) -> bool {
        let c = &self.0;
        c.contains(pos)
            && (pos.x == c.min.x
                || pos.x == c.max.x
                || pos.y == c.min.y
                || pos.y == c.max.y
                || pos.z == c.min.z
                || pos.z == c.max.z)
    }

    fn for_each_span(&self, y: i32, z: i32, x_min: i32, x_max: i32, f: &mut dyn FnMut(i32, i32)) {
        let c = &self.0;
        let full = z == c.min.z || z == c.max.z || y == c.min.y || y == c.max.y;
        shell_spans(c, full, y, z, x_min, x_max, f);
    }
}

fn shell_spans(
    c: &Cuboid,
    full_row: bool,
    y: i32,
    z: i32,
    x_min: i32,
    x_max: i32,
    f: &mut dyn FnMut(i32, i32),
) {
    if !(c.min.y..=c.max.y).contains(&y) || !(c.min.z..=c.max.z).contains(&z) {
        return;
    }
    if full_row {
        return c.for_each_span(y, z, x_min, x_max, f);
    }
    for x in [c.min.x, c.max.x] {
        if (x_min..=x_max).contains(&x) {
            f(x, x);
        }
        if c.min.x == c.max.x {
            break;
        }
    }
}

fn default_spans<R: Region + ?Sized>(
    region: &R,
    y: i32,
    z: i32,
    x_min: i32,
    x_max: i32,
    f: &mut dyn FnMut(i32, i32),
) {
    let bounds = region.bounds();
    let (from, to) = (x_min.max(bounds.min.x), x_max.min(bounds.max.x));
    let mut start = None;
    for x in from..=to {
        let inside = region.contains(BlockPos::new(x, y, z));
        match (inside, start) {
            (true, None) => start = Some(x),
            (false, Some(s)) => {
                f(s, x - 1);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        f(s, to);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brute_volume(region: &dyn Region) -> u64 {
        region
            .bounds()
            .positions()
            .filter(|pos| region.contains(*pos))
            .count() as u64
    }

    fn assert_spans_match_contains(region: &dyn Region) {
        let bounds = region.bounds();
        for y in bounds.min.y..=bounds.max.y {
            for z in bounds.min.z..=bounds.max.z {
                let mut from_spans = Vec::new();
                region.for_each_span(y, z, bounds.min.x - 3, bounds.max.x + 3, &mut |a, b| {
                    from_spans.extend(a..=b);
                });
                let expected: Vec<i32> = (bounds.min.x - 3..=bounds.max.x + 3)
                    .filter(|x| region.contains(BlockPos::new(*x, y, z)))
                    .collect();
                assert_eq!(from_spans, expected, "row y={y} z={z} of {region:?}");
            }
        }
    }

    #[test]
    fn cuboid_normalizes_corners_and_counts_volume() {
        let cuboid = Cuboid::new(BlockPos::new(5, 1, -2), BlockPos::new(-1, 3, 2));
        assert_eq!(cuboid.min, BlockPos::new(-1, 1, -2));
        assert_eq!(cuboid.max, BlockPos::new(5, 3, 2));
        assert_eq!(cuboid.volume(), 7 * 3 * 5);
        assert_spans_match_contains(&cuboid);
    }

    #[test]
    fn expand_and_contract_move_one_face() {
        let cuboid = Cuboid::new(BlockPos::new(0, 0, 0), BlockPos::new(3, 3, 3));
        assert_eq!(cuboid.expanded(Direction::Up, 2).max.y, 5);
        assert_eq!(cuboid.expanded(Direction::West, 2).min.x, -2);
        assert_eq!(cuboid.contracted(Direction::Up, 1).min.y, 1);
        assert_eq!(cuboid.contracted(Direction::North, 1).max.z, 2);
        let collapsed = cuboid.contracted(Direction::East, 10);
        assert_eq!((collapsed.min.x, collapsed.max.x), (3, 3));
    }

    #[test]
    fn sections_cover_every_block() {
        let cuboid = Cuboid::new(BlockPos::new(-1, 0, 15), BlockPos::new(16, 16, 16));
        let sections: Vec<_> = cuboid.sections().collect();
        assert_eq!(sections.len(), 3 * 2 * 2);
        for pos in cuboid.positions() {
            assert!(sections.contains(&pos.section()));
        }
    }

    #[test]
    fn sphere_spans_match_contains_and_brute_force_volume() {
        for radius in [0.0, 1.0, 2.5, 5.0] {
            let sphere = Ellipsoid::sphere(BlockPos::new(3, -7, 11), radius);
            assert_spans_match_contains(&sphere);
            assert_eq!(sphere.volume(), brute_volume(&sphere));
        }
        assert_eq!(Ellipsoid::sphere(BlockPos::ZERO, 0.0).volume(), 1);
        assert_eq!(Ellipsoid::sphere(BlockPos::ZERO, 1.0).volume(), 19);
    }

    #[test]
    fn hollow_shapes_keep_only_the_shell() {
        let sphere = Ellipsoid::sphere(BlockPos::ZERO, 4.0).hollow();
        assert!(!sphere.contains(BlockPos::ZERO));
        assert!(sphere.contains(BlockPos::new(4, 0, 0)));
        assert_spans_match_contains(&sphere);

        let cylinder = Cylinder::new(BlockPos::ZERO, 3.0, 4).hollow();
        assert!(!cylinder.contains(BlockPos::new(0, 1, 0)));
        assert!(cylinder.contains(BlockPos::new(3, 1, 0)));
        assert_spans_match_contains(&cylinder);
    }

    #[test]
    fn cylinder_spans_match_contains() {
        let cylinder = Cylinder::new(BlockPos::new(1, 2, 3), 4.0, 3);
        assert_eq!(cylinder.bounds().min.y, 2);
        assert_eq!(cylinder.bounds().max.y, 4);
        assert_spans_match_contains(&cylinder);
        assert_eq!(cylinder.volume(), brute_volume(&cylinder));
    }

    #[test]
    fn walls_and_faces_are_shells() {
        let cuboid = Cuboid::new(BlockPos::ZERO, BlockPos::new(4, 3, 5));
        let walls = Walls(cuboid);
        let faces = Faces(cuboid);
        assert_spans_match_contains(&walls);
        assert_spans_match_contains(&faces);
        assert_eq!(walls.volume(), brute_volume(&walls));
        assert_eq!(walls.volume(), 4 * (2 * 5 + 2 * 4));
        assert_eq!(faces.volume(), cuboid.volume() - 3 * 2 * 4);

        let thin = Walls(Cuboid::new(BlockPos::ZERO, BlockPos::new(0, 1, 3)));
        assert_spans_match_contains(&thin);
        assert_eq!(thin.volume(), 8);
    }
}
