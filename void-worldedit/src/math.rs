use std::fmt;
use std::ops::{Add, AddAssign, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub const ZERO: Self = Self::new(0, 0, 0);

    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    pub fn offset(self, dx: i32, dy: i32, dz: i32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.z + dz)
    }

    pub fn min(self, other: Self) -> Self {
        Self::new(
            self.x.min(other.x),
            self.y.min(other.y),
            self.z.min(other.z),
        )
    }

    pub fn max(self, other: Self) -> Self {
        Self::new(
            self.x.max(other.x),
            self.y.max(other.y),
            self.z.max(other.z),
        )
    }

    pub fn section(self) -> SectionPos {
        SectionPos::new(self.x >> 4, self.y >> 4, self.z >> 4)
    }

    pub fn section_index(self) -> usize {
        section_index(
            (self.x & 15) as usize,
            (self.y & 15) as usize,
            (self.z & 15) as usize,
        )
    }

    pub fn center(self) -> Vec3 {
        Vec3::new(
            f64::from(self.x) + 0.5,
            f64::from(self.y) + 0.5,
            f64::from(self.z) + 0.5,
        )
    }

    pub fn distance_squared(self, other: Self) -> i64 {
        let dx = i64::from(self.x) - i64::from(other.x);
        let dy = i64::from(self.y) - i64::from(other.y);
        let dz = i64::from(self.z) - i64::from(other.z);
        dx * dx + dy * dy + dz * dz
    }

    /// Clamped inside the vanilla world border on every axis, so that sizes,
    /// offsets and sums of two positions never overflow an `i32`.
    pub fn clamped(self) -> Self {
        let clamp = |v: i32| v.clamp(-WORLD_LIMIT, WORLD_LIMIT);
        Self::new(clamp(self.x), clamp(self.y), clamp(self.z))
    }
}

impl fmt::Display for BlockPos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {}, {})", self.x, self.y, self.z)
    }
}

impl Add for BlockPos {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl AddAssign for BlockPos {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sub for BlockPos {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl Neg for BlockPos {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z)
    }
}

impl Mul<i32> for BlockPos {
    type Output = Self;
    fn mul(self, rhs: i32) -> Self {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}

pub const SECTION_VOLUME: usize = 4096;

pub const WORLD_LIMIT: i32 = 30_000_000;

pub const fn section_index(x: usize, y: usize, z: usize) -> usize {
    (y << 8) | (z << 4) | x
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SectionPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl SectionPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    pub fn min_block(self) -> BlockPos {
        BlockPos::new(self.x << 4, self.y << 4, self.z << 4)
    }

    pub fn block(self, index: usize) -> BlockPos {
        self.min_block().offset(
            (index & 15) as i32,
            (index >> 8) as i32,
            ((index >> 4) & 15) as i32,
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn from_rotation(yaw: f32, pitch: f32) -> Self {
        let (yaw, pitch) = (f64::from(yaw).to_radians(), f64::from(pitch).to_radians());
        Self::new(
            -yaw.sin() * pitch.cos(),
            -pitch.sin(),
            yaw.cos() * pitch.cos(),
        )
    }

    pub fn block(self) -> BlockPos {
        BlockPos::new(
            self.x.floor() as i32,
            self.y.floor() as i32,
            self.z.floor() as i32,
        )
    }

    pub fn length(self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }
}

impl Add for Vec3 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl Mul<f64> for Vec3 {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
    X,
    Y,
    Z,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    North,
    South,
    East,
    West,
    Up,
    Down,
}

impl Direction {
    pub const ALL: [Direction; 6] = [
        Direction::North,
        Direction::South,
        Direction::East,
        Direction::West,
        Direction::Up,
        Direction::Down,
    ];

    pub fn vector(self) -> BlockPos {
        match self {
            Direction::North => BlockPos::new(0, 0, -1),
            Direction::South => BlockPos::new(0, 0, 1),
            Direction::East => BlockPos::new(1, 0, 0),
            Direction::West => BlockPos::new(-1, 0, 0),
            Direction::Up => BlockPos::new(0, 1, 0),
            Direction::Down => BlockPos::new(0, -1, 0),
        }
    }

    pub fn axis(self) -> Axis {
        match self {
            Direction::East | Direction::West => Axis::X,
            Direction::Up | Direction::Down => Axis::Y,
            Direction::North | Direction::South => Axis::Z,
        }
    }

    pub fn opposite(self) -> Self {
        match self {
            Direction::North => Direction::South,
            Direction::South => Direction::North,
            Direction::East => Direction::West,
            Direction::West => Direction::East,
            Direction::Up => Direction::Down,
            Direction::Down => Direction::Up,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Direction::North => "north",
            Direction::South => "south",
            Direction::East => "east",
            Direction::West => "west",
            Direction::Up => "up",
            Direction::Down => "down",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.name() == name)
    }

    /// The direction a player looks towards: up/down past 67.5° of pitch,
    /// else the nearest horizontal direction.
    pub fn facing(yaw: f32, pitch: f32) -> Self {
        if pitch <= -67.5 {
            return Direction::Up;
        }
        if pitch >= 67.5 {
            return Direction::Down;
        }
        match (yaw.rem_euclid(360.0) / 90.0).round() as i32 % 4 {
            0 => Direction::South,
            1 => Direction::West,
            2 => Direction::North,
            _ => Direction::East,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_index_round_trips_through_section_block() {
        assert_eq!(
            BlockPos::new(i32::MIN, 5, i32::MAX).clamped(),
            BlockPos::new(-WORLD_LIMIT, 5, WORLD_LIMIT)
        );
        let pos = BlockPos::new(-17, -61, 35);
        let section = pos.section();
        assert_eq!(section, SectionPos::new(-2, -4, 2));
        assert_eq!(section.block(pos.section_index()), pos);
    }

    #[test]
    fn facing_follows_minecraft_yaw_convention() {
        assert_eq!(Direction::facing(0.0, 0.0), Direction::South);
        assert_eq!(Direction::facing(90.0, 0.0), Direction::West);
        assert_eq!(Direction::facing(-180.0, 10.0), Direction::North);
        assert_eq!(Direction::facing(-90.0, 0.0), Direction::East);
        assert_eq!(Direction::facing(10.0, -80.0), Direction::Up);
        assert_eq!(Direction::facing(10.0, 90.0), Direction::Down);
    }

    #[test]
    fn look_vector_matches_facing() {
        let south = Vec3::from_rotation(0.0, 0.0);
        assert!((south.z - 1.0).abs() < 1e-9 && south.x.abs() < 1e-9);
        let down = Vec3::from_rotation(0.0, 90.0);
        assert!((down.y + 1.0).abs() < 1e-9);
    }
}
