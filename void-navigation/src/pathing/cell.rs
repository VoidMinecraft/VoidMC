#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CellKind {
    Normal = 0,
    Water = 1,
    Lava = 2,
    Hazard = 3,
    Stair = 4,
    Unloaded = 5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Side {
    North = 0,
    East = 1,
    South = 2,
    West = 3,
}

impl Side {
    pub const ALL: [Side; 4] = [Side::North, Side::East, Side::South, Side::West];

    pub const fn opposite(self) -> Side {
        match self {
            Side::North => Side::South,
            Side::East => Side::West,
            Side::South => Side::North,
            Side::West => Side::East,
        }
    }

    pub const fn mask(self) -> u8 {
        1 << self as u8
    }

    pub const fn of_step(dx: i32, dz: i32) -> Option<Side> {
        match (dx, dz) {
            (0, -1) => Some(Side::North),
            (1, 0) => Some(Side::East),
            (0, 1) => Some(Side::South),
            (-1, 0) => Some(Side::West),
            _ => None,
        }
    }
}

/// One block of navigation data packed in 16 bits: the vertical extent of the
/// collision that crosses the middle of the block (in sixteenths, `top` up to
/// 24 for fences and walls), the sides blocked by thin panels such as doors
/// and panes, and a terrain kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Cell(u16);

const TOP_MASK: u16 = 0b1_1111;
const BOTTOM_SHIFT: u16 = 5;
const BOTTOM_MASK: u16 = 0b1111;
const EDGE_SHIFT: u16 = 9;
const EDGE_MASK: u16 = 0b1111;
const KIND_SHIFT: u16 = 13;

pub const MAX_TOP: u8 = 24;

impl Cell {
    pub const EMPTY: Cell = Cell(0);
    pub const FULL: Cell = Cell::solid(0, 16);
    pub const UNLOADED: Cell = Cell::EMPTY.with_kind(CellKind::Unloaded);

    pub const fn solid(bottom: u8, top: u8) -> Cell {
        debug_assert!(bottom < top && top <= MAX_TOP && bottom < 16);
        Cell(top as u16 & TOP_MASK | (bottom as u16 & BOTTOM_MASK) << BOTTOM_SHIFT)
    }

    pub const fn with_kind(self, kind: CellKind) -> Cell {
        Cell(self.0 & !(0b111 << KIND_SHIFT) | (kind as u16) << KIND_SHIFT)
    }

    pub const fn with_edges(self, edges: u8) -> Cell {
        Cell(self.0 & !(EDGE_MASK << EDGE_SHIFT) | (edges as u16 & EDGE_MASK) << EDGE_SHIFT)
    }

    pub const fn bits(self) -> u16 {
        self.0
    }

    pub const fn from_bits(bits: u16) -> Cell {
        Cell(bits)
    }

    pub const fn top(self) -> i32 {
        (self.0 & TOP_MASK) as i32
    }

    pub const fn bottom(self) -> i32 {
        (self.0 >> BOTTOM_SHIFT & BOTTOM_MASK) as i32
    }

    pub const fn has_collision(self) -> bool {
        self.0 & TOP_MASK != 0
    }

    pub const fn edges(self) -> u8 {
        (self.0 >> EDGE_SHIFT & EDGE_MASK) as u8
    }

    pub const fn blocks_side(self, side: Side) -> bool {
        self.edges() & side.mask() != 0
    }

    pub const fn kind(self) -> CellKind {
        match self.0 >> KIND_SHIFT {
            1 => CellKind::Water,
            2 => CellKind::Lava,
            3 => CellKind::Hazard,
            4 => CellKind::Stair,
            5 => CellKind::Unloaded,
            _ => CellKind::Normal,
        }
    }

    pub const fn is_unloaded(self) -> bool {
        self.0 >> KIND_SHIFT == CellKind::Unloaded as u16
    }

    /// Whether this cell's collision overlaps `[lo, hi)`, both in absolute
    /// sixteenths with this cell's bottom at `base`.
    #[inline]
    pub const fn overlaps(self, base: i32, lo: i32, hi: i32) -> bool {
        self.has_collision() && base + self.bottom() < hi && base + self.top() > lo
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_every_field_independently() {
        let cell = Cell::solid(8, 24)
            .with_edges(Side::North.mask() | Side::West.mask())
            .with_kind(CellKind::Hazard);
        assert_eq!(cell.bottom(), 8);
        assert_eq!(cell.top(), 24);
        assert!(cell.blocks_side(Side::North));
        assert!(cell.blocks_side(Side::West));
        assert!(!cell.blocks_side(Side::East));
        assert_eq!(cell.kind(), CellKind::Hazard);
        assert_eq!(cell.with_kind(CellKind::Normal).top(), 24);
    }

    #[test]
    fn empty_and_unloaded_have_no_collision() {
        assert!(!Cell::EMPTY.has_collision());
        assert!(!Cell::UNLOADED.has_collision());
        assert!(Cell::UNLOADED.is_unloaded());
        assert_eq!(Cell::EMPTY.kind(), CellKind::Normal);
    }

    #[test]
    fn overlap_is_half_open() {
        let slab = Cell::solid(0, 8);
        assert!(slab.overlaps(64 * 16, 64 * 16, 64 * 16 + 1));
        assert!(!slab.overlaps(64 * 16, 64 * 16 + 8, 64 * 16 + 40));
    }
}
