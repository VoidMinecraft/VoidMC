use std::ops::{Add, AddAssign, Mul, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    pub const fn offset(self, dx: i32, dy: i32, dz: i32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.z + dz)
    }

    pub fn containing(point: Vec3) -> Self {
        Self::new(
            point.x.floor() as i32,
            point.y.floor() as i32,
            point.z.floor() as i32,
        )
    }

    pub fn bottom_center(self) -> Vec3 {
        Vec3::new(self.x as f64 + 0.5, self.y as f64, self.z as f64 + 0.5)
    }

    pub fn center(self) -> Vec3 {
        Vec3::new(
            self.x as f64 + 0.5,
            self.y as f64 + 0.5,
            self.z as f64 + 0.5,
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
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);

    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn length(self) -> f64 {
        self.length_squared().sqrt()
    }

    pub fn length_squared(self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    pub fn horizontal_length(self) -> f64 {
        (self.x * self.x + self.z * self.z).sqrt()
    }

    pub fn distance(self, other: Self) -> f64 {
        (self - other).length()
    }

    pub fn distance_squared(self, other: Self) -> f64 {
        (self - other).length_squared()
    }

    pub fn horizontal_distance(self, other: Self) -> f64 {
        (self - other).horizontal_length()
    }

    pub fn normalize_or_zero(self) -> Self {
        let length = self.length();
        if length > f64::EPSILON {
            self * (1.0 / length)
        } else {
            Self::ZERO
        }
    }

    pub fn lerp(self, other: Self, t: f64) -> Self {
        self + (other - self) * t
    }
}

impl Add for Vec3 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl AddAssign for Vec3 {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sub for Vec3 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl Mul<f64> for Vec3 {
    type Output = Self;

    fn mul(self, rhs: f64) -> Self {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}

impl From<[f64; 3]> for Vec3 {
    fn from([x, y, z]: [f64; 3]) -> Self {
        Self::new(x, y, z)
    }
}

impl From<(f64, f64, f64)> for Vec3 {
    fn from((x, y, z): (f64, f64, f64)) -> Self {
        Self::new(x, y, z)
    }
}

impl From<BlockPos> for Vec3 {
    fn from(pos: BlockPos) -> Self {
        pos.bottom_center()
    }
}

pub fn yaw_towards(dx: f64, dz: f64) -> f32 {
    let yaw = (-dx).atan2(dz).to_degrees() as f32;
    if yaw < 0.0 { yaw + 360.0 } else { yaw }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn containing_floors_negative_coordinates() {
        assert_eq!(
            BlockPos::containing(Vec3::new(-0.5, 64.0, -16.01)),
            BlockPos::new(-1, 64, -17)
        );
    }

    #[test]
    fn yaw_matches_protocol_convention() {
        assert_eq!(yaw_towards(0.0, 1.0), 0.0);
        assert_eq!(yaw_towards(-1.0, 0.0), 90.0);
        assert_eq!(yaw_towards(0.0, -1.0), 180.0);
        assert_eq!(yaw_towards(1.0, 0.0), 270.0);
    }
}
