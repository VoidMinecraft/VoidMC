//! The part of the world the map shows, as a grid of cells.

use std::num::{NonZeroU8, NonZeroU16};

/// Furthest block a view centre can reach, the vanilla world border.
pub const WORLD_LIMIT: i32 = 29_999_984;

/// Share of the view that one pan step moves by.
const PAN_DIVISOR: u16 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    North,
    South,
    East,
    West,
}

/// A view of `cols` x `rows` cells, each `scale` blocks wide.
///
/// The centre always sits on the cell grid, so a cell keeps the same blocks,
/// and the same colour, however the player reached it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    center_x: i32,
    center_z: i32,
    cols: NonZeroU16,
    rows: NonZeroU16,
    scale: NonZeroU8,
}

impl Window {
    pub fn new(
        center_x: i32,
        center_z: i32,
        cols: NonZeroU16,
        rows: NonZeroU16,
        scale: NonZeroU8,
    ) -> Self {
        let mut window = Self {
            center_x: 0,
            center_z: 0,
            cols,
            rows,
            scale,
        };
        window.center_on(center_x, center_z);
        window
    }

    pub fn cols(&self) -> u16 {
        self.cols.get()
    }

    pub fn rows(&self) -> u16 {
        self.rows.get()
    }

    /// Blocks per cell.
    pub fn scale(&self) -> u8 {
        self.scale.get()
    }

    pub fn center(&self) -> (i32, i32) {
        (self.center_x, self.center_z)
    }

    fn step(&self) -> i32 {
        i32::from(self.scale())
    }

    /// Lowest corner of a cell. `col` and `row` may lie outside the view.
    pub fn block_at(&self, col: u16, row: u16) -> (i32, i32) {
        let (x, z) = self.origin();
        (
            x + i32::from(col) * self.step(),
            z + i32::from(row) * self.step(),
        )
    }

    fn origin(&self) -> (i32, i32) {
        (
            self.center_x - i32::from(self.cols() / 2) * self.step(),
            self.center_z - i32::from(self.rows() / 2) * self.step(),
        )
    }

    /// The cell holding a block, or `None` when the view does not show it.
    pub fn cell_at(&self, x: i32, z: i32) -> Option<(u16, u16)> {
        let (origin_x, origin_z) = self.origin();
        let col = u16::try_from((x - origin_x).div_euclid(self.step())).ok()?;
        let row = u16::try_from((z - origin_z).div_euclid(self.step())).ok()?;
        (col < self.cols() && row < self.rows()).then_some((col, row))
    }

    /// Moves to a block, snapped to the cell grid and kept inside the world.
    pub fn center_on(&mut self, x: i32, z: i32) {
        let snap = |value: i32| {
            value
                .clamp(-WORLD_LIMIT, WORLD_LIMIT)
                .div_euclid(self.step())
                * self.step()
        };
        (self.center_x, self.center_z) = (snap(x), snap(z));
    }

    /// Moves by a quarter of the view, and by at least one cell.
    pub fn pan(&mut self, direction: Direction) {
        let across = i32::from((self.cols() / PAN_DIVISOR).max(1)) * self.step();
        let along = i32::from((self.rows() / PAN_DIVISOR).max(1)) * self.step();
        let (x, z) = self.center();
        match direction {
            Direction::North => self.center_on(x, z - along),
            Direction::South => self.center_on(x, z + along),
            Direction::East => self.center_on(x + across, z),
            Direction::West => self.center_on(x - across, z),
        }
    }

    /// Centres on a cell of the current view. Refuses a cell outside it,
    /// because cells come from client clicks.
    pub fn recenter(&mut self, col: u16, row: u16) -> bool {
        if col >= self.cols() || row >= self.rows() {
            return false;
        }
        let (x, z) = self.block_at(col, row);
        self.center_on(x, z);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::window;

    #[test]
    fn the_centre_snaps_to_the_cell_grid() {
        assert_eq!(window(13, -13, 8, 6, 4).center(), (12, -16));
    }

    #[test]
    fn panning_moves_a_quarter_of_the_view_and_comes_back() {
        let start = window(0, 0, 96, 80, 4);
        let mut view = start;
        view.pan(Direction::East);
        assert_eq!(view.center(), (96, 0));
        view.pan(Direction::South);
        assert_eq!(view.center(), (96, 80));
        view.pan(Direction::West);
        view.pan(Direction::North);
        assert_eq!(view, start);
    }

    #[test]
    fn a_tiny_view_still_pans_by_one_cell() {
        let mut view = window(0, 0, 1, 1, 4);
        view.pan(Direction::East);
        assert_eq!(view.center(), (4, 0));
    }

    #[test]
    fn panning_stops_at_the_world_border() {
        let mut view = window(WORLD_LIMIT, 0, 96, 80, 4);
        let center = view.center();
        view.pan(Direction::East);
        assert_eq!(view.center(), center);
    }

    #[test]
    fn extreme_requests_stay_inside_the_world() {
        let view = window(i32::MAX, i32::MIN, u16::MAX, u16::MAX, u8::MAX);
        let (x, z) = view.center();
        assert!(x <= WORLD_LIMIT);
        assert!(z >= -WORLD_LIMIT - i32::from(u8::MAX));
        assert!(view.block_at(u16::MAX, u16::MAX).0 > x);
    }

    #[test]
    fn cells_and_blocks_convert_both_ways_around_negative_coordinates() {
        let view = window(-40, -40, 8, 6, 4);
        for row in 0..6 {
            for col in 0..8 {
                let (x, z) = view.block_at(col, row);
                assert_eq!(view.cell_at(x, z), Some((col, row)));
                assert_eq!(view.cell_at(x + 3, z + 3), Some((col, row)));
            }
        }
    }

    #[test]
    fn blocks_outside_the_view_have_no_cell() {
        let view = window(0, 0, 8, 6, 4);
        let (x, z) = view.block_at(0, 0);
        assert_eq!(view.cell_at(x - 1, z), None);
        assert_eq!(view.cell_at(x, z - 1), None);
        let (x, z) = view.block_at(8, 6);
        assert_eq!(view.cell_at(x, z - 1), None);
        assert_eq!(view.cell_at(x - 1, z), None);
    }

    #[test]
    fn recentring_brings_the_clicked_cell_to_the_middle() {
        let mut view = window(0, 0, 8, 6, 4);
        let block = view.block_at(1, 5);
        assert!(view.recenter(1, 5));
        assert_eq!(view.center(), block);
        assert_eq!(view.cell_at(block.0, block.1), Some((4, 3)));
    }

    #[test]
    fn recentring_refuses_a_cell_outside_the_view() {
        let start = window(0, 0, 8, 6, 4);
        let mut view = start;
        assert!(!view.recenter(8, 0));
        assert!(!view.recenter(0, 6));
        assert_eq!(view, start);
    }
}
