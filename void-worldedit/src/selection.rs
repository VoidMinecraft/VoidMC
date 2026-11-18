use std::fmt;
use std::sync::Arc;

use crate::math::{BlockPos, Direction};
use crate::region::{Cuboid, Cylinder, Ellipsoid, Region};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SelectionShape {
    #[default]
    Cuboid,
    /// `pos1` is the center, `pos2` any point on the surface.
    Sphere,
    /// `pos1` is the base center, `pos2` sets the radius and the height.
    Cylinder,
}

impl SelectionShape {
    pub fn name(self) -> &'static str {
        match self {
            SelectionShape::Cuboid => "cuboid",
            SelectionShape::Sphere => "sphere",
            SelectionShape::Cylinder => "cylinder",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionError {
    Incomplete,
    NotCuboid,
}

impl fmt::Display for SelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SelectionError::Incomplete => {
                f.write_str("Make a region selection first (set both positions).")
            }
            SelectionError::NotCuboid => f.write_str("This only works on a cuboid selection."),
        }
    }
}

impl std::error::Error for SelectionError {}

/// Two positions and the shape they describe. Every change bumps
/// [`revision`](Self::revision), so a renderer can redraw only on change.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    shape: SelectionShape,
    pos1: Option<BlockPos>,
    pos2: Option<BlockPos>,
    revision: u64,
}

impl Selection {
    pub fn new(shape: SelectionShape) -> Self {
        Self {
            shape,
            ..Self::default()
        }
    }

    pub fn shape(&self) -> SelectionShape {
        self.shape
    }

    pub fn pos1(&self) -> Option<BlockPos> {
        self.pos1
    }

    pub fn pos2(&self) -> Option<BlockPos> {
        self.pos2
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn is_complete(&self) -> bool {
        self.pos1.is_some() && self.pos2.is_some()
    }

    pub fn set_shape(&mut self, shape: SelectionShape) {
        self.shape = shape;
        self.pos1 = None;
        self.pos2 = None;
        self.revision += 1;
    }

    pub fn set_pos1(&mut self, pos: BlockPos) {
        self.pos1 = Some(pos.clamped());
        self.revision += 1;
    }

    pub fn set_pos2(&mut self, pos: BlockPos) {
        self.pos2 = Some(pos.clamped());
        self.revision += 1;
    }

    pub fn clear(&mut self) {
        self.pos1 = None;
        self.pos2 = None;
        self.revision += 1;
    }

    fn points(&self) -> Result<(BlockPos, BlockPos), SelectionError> {
        self.pos1.zip(self.pos2).ok_or(SelectionError::Incomplete)
    }

    pub fn region(&self) -> Result<Arc<dyn Region>, SelectionError> {
        let (a, b) = self.points()?;
        Ok(match self.shape {
            SelectionShape::Cuboid => Arc::new(Cuboid::new(a, b)),
            SelectionShape::Sphere => Arc::new(Ellipsoid::sphere(
                a,
                (a.distance_squared(b) as f64).sqrt().round(),
            )),
            SelectionShape::Cylinder => {
                let dx = f64::from(b.x - a.x);
                let dz = f64::from(b.z - a.z);
                let radius = (dx * dx + dz * dz).sqrt().round();
                let base = BlockPos::new(a.x, a.y.min(b.y), a.z);
                Arc::new(Cylinder::new(base, radius, (b.y - a.y).abs() + 1))
            }
        })
    }

    pub fn cuboid(&self) -> Result<Cuboid, SelectionError> {
        let (a, b) = self.points()?;
        match self.shape {
            SelectionShape::Cuboid => Ok(Cuboid::new(a, b)),
            _ => Err(SelectionError::NotCuboid),
        }
    }

    pub fn bounds(&self) -> Result<Cuboid, SelectionError> {
        Ok(self.region()?.bounds())
    }

    fn set_cuboid(&mut self, cuboid: Cuboid) {
        let (a, b) = self.points().expect("cuboid selection is complete");
        let pick = |pa: i32, pb: i32, lo: i32, hi: i32| if pa <= pb { (lo, hi) } else { (hi, lo) };
        let (ax, bx) = pick(a.x, b.x, cuboid.min.x, cuboid.max.x);
        let (ay, by) = pick(a.y, b.y, cuboid.min.y, cuboid.max.y);
        let (az, bz) = pick(a.z, b.z, cuboid.min.z, cuboid.max.z);
        self.pos1 = Some(BlockPos::new(ax, ay, az).clamped());
        self.pos2 = Some(BlockPos::new(bx, by, bz).clamped());
        self.revision += 1;
    }

    pub fn expand(&mut self, direction: Direction, amount: i32) -> Result<(), SelectionError> {
        let amount = amount.clamp(0, 2 * crate::math::WORLD_LIMIT);
        let cuboid = self.cuboid()?;
        self.set_cuboid(cuboid.expanded(direction, amount));
        Ok(())
    }

    pub fn contract(&mut self, direction: Direction, amount: i32) -> Result<(), SelectionError> {
        let amount = amount.clamp(0, 2 * crate::math::WORLD_LIMIT);
        let cuboid = self.cuboid()?;
        self.set_cuboid(cuboid.contracted(direction, amount));
        Ok(())
    }

    pub fn expand_vertical(&mut self, (min_y, max_y): (i32, i32)) -> Result<(), SelectionError> {
        let cuboid = self.cuboid()?;
        self.set_cuboid(Cuboid {
            min: BlockPos::new(cuboid.min.x, min_y, cuboid.min.z),
            max: BlockPos::new(cuboid.max.x, max_y, cuboid.max.z),
        });
        Ok(())
    }

    pub fn shift(&mut self, offset: BlockPos) -> Result<(), SelectionError> {
        let (a, b) = self.points()?;
        let offset = offset.clamped();
        self.pos1 = Some((a + offset).clamped());
        self.pos2 = Some((b + offset).clamped());
        self.revision += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selection(a: (i32, i32, i32), b: (i32, i32, i32)) -> Selection {
        let mut selection = Selection::new(SelectionShape::Cuboid);
        selection.set_pos1(BlockPos::new(a.0, a.1, a.2));
        selection.set_pos2(BlockPos::new(b.0, b.1, b.2));
        selection
    }

    #[test]
    fn incomplete_selection_has_no_region() {
        let mut selection = Selection::default();
        assert_eq!(selection.region().unwrap_err(), SelectionError::Incomplete);
        selection.set_pos1(BlockPos::ZERO);
        assert!(!selection.is_complete());
    }

    #[test]
    fn expand_keeps_each_corner_on_its_side() {
        let mut selection = selection((5, 0, 0), (0, 3, 3));
        selection.expand(Direction::East, 2).unwrap();
        assert_eq!(selection.pos1(), Some(BlockPos::new(7, 0, 0)));
        assert_eq!(selection.pos2(), Some(BlockPos::new(0, 3, 3)));
        selection.contract(Direction::Up, 1).unwrap();
        assert_eq!(selection.cuboid().unwrap().min.y, 1);
        selection.expand_vertical((-64, 319)).unwrap();
        assert_eq!(selection.cuboid().unwrap().min.y, -64);
        assert_eq!(selection.cuboid().unwrap().max.y, 319);
    }

    #[test]
    fn huge_coordinates_are_clamped_and_never_overflow() {
        let mut selection = selection((-2_000_000_000, 64, 0), (2_000_000_000, 64, 0));
        assert_eq!(selection.cuboid().unwrap().size().x, 60_000_001);
        selection.expand(Direction::East, i32::MAX).unwrap();
        selection.shift(BlockPos::new(i32::MAX, 0, 0)).unwrap();
        let region = selection.region().unwrap();
        assert_eq!(region.bounds().max.x, crate::math::WORLD_LIMIT);

        let mut sphere = Selection::new(SelectionShape::Sphere);
        sphere.set_pos1(BlockPos::new(1_000_000_000, 0, 0));
        sphere.set_pos2(BlockPos::new(-1_000_000_000, 0, 0));
        assert!(sphere.bounds().unwrap().cuboid_volume() > 1 << 60);
    }

    #[test]
    fn shift_moves_both_positions_and_bumps_revision() {
        let mut selection = selection((0, 0, 0), (1, 1, 1));
        let revision = selection.revision();
        selection.shift(BlockPos::new(0, 5, 0)).unwrap();
        assert_eq!(selection.pos1(), Some(BlockPos::new(0, 5, 0)));
        assert!(selection.revision() > revision);
    }

    #[test]
    fn sphere_and_cylinder_regions_from_two_points() {
        let mut sphere = Selection::new(SelectionShape::Sphere);
        sphere.set_pos1(BlockPos::new(0, 64, 0));
        sphere.set_pos2(BlockPos::new(3, 64, 4));
        assert_eq!(
            sphere.bounds().unwrap(),
            Cuboid::new(BlockPos::new(-5, 59, -5), BlockPos::new(5, 69, 5))
        );
        assert_eq!(
            sphere.expand(Direction::Up, 1),
            Err(SelectionError::NotCuboid)
        );

        let mut cylinder = Selection::new(SelectionShape::Cylinder);
        cylinder.set_pos1(BlockPos::new(0, 60, 0));
        cylinder.set_pos2(BlockPos::new(2, 63, 0));
        let bounds = cylinder.bounds().unwrap();
        assert_eq!((bounds.min.y, bounds.max.y), (60, 63));
        assert_eq!((bounds.min.x, bounds.max.x), (-2, 2));
    }
}
