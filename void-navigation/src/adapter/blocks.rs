use std::ops::RangeInclusive;

use voidmc::world::is_solid_block_state;
use voidmc_data::v26_1_2::{blocks, shapes, state};

use crate::pathing::{Cell, CellKind, MAX_TOP, Side};

/// How block states turn into navigation cells.
///
/// `FullBlocks` mirrors VoidMC's current entity physics, which collides with
/// every block other than air and water as a full cube: paths then match what
/// mobs can physically do. `Vanilla` reads the real collision shapes (slabs,
/// stairs, fences, doors, carpets...) and is the model to use once the physics
/// becomes shape-accurate. Hazards, water and lava are classified in both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BlockModel {
    #[default]
    FullBlocks,
    Vanilla,
}

/// One [`Cell`] per block state, built once per model.
#[derive(Clone, Debug)]
pub struct CellTable {
    cells: Box<[Cell]>,
    model: BlockModel,
}

impl CellTable {
    pub fn new(model: BlockModel) -> Self {
        let states = blocks::BLOCK_IDS
            .iter()
            .map(|&(_, state)| state)
            .max()
            .unwrap_or(0)
            + 1;
        let cells = (0..states).map(|state| classify(model, state)).collect();
        Self { cells, model }
    }

    pub fn model(&self) -> BlockModel {
        self.model
    }

    #[inline]
    pub fn get(&self, state: i32) -> Cell {
        match usize::try_from(state)
            .ok()
            .and_then(|index| self.cells.get(index))
        {
            Some(&cell) => cell,
            None if state < 0 => Cell::FULL,
            None => classify(self.model, state),
        }
    }
}

fn range(min: i32, max: i32) -> RangeInclusive<i32> {
    min..=max
}

fn special_kind(id: i32) -> Option<CellKind> {
    let water = [
        range(state::Water::MIN_STATE_ID, state::Water::MAX_STATE_ID),
        range(state::Kelp::MIN_STATE_ID, state::Kelp::MAX_STATE_ID),
        range(blocks::KELP_PLANT, blocks::KELP_PLANT),
        range(blocks::SEAGRASS, blocks::SEAGRASS),
        range(
            state::TallSeagrass::MIN_STATE_ID,
            state::TallSeagrass::MAX_STATE_ID,
        ),
        range(
            state::BubbleColumn::MIN_STATE_ID,
            state::BubbleColumn::MAX_STATE_ID,
        ),
    ];
    if water.iter().any(|r| r.contains(&id)) {
        return Some(CellKind::Water);
    }
    if range(state::Lava::MIN_STATE_ID, state::Lava::MAX_STATE_ID).contains(&id) {
        return Some(CellKind::Lava);
    }
    let hazards = [
        range(state::Fire::MIN_STATE_ID, state::Fire::MAX_STATE_ID),
        range(blocks::SOUL_FIRE, blocks::SOUL_FIRE),
        range(state::Campfire::MIN_STATE_ID, state::Campfire::MAX_STATE_ID),
        range(
            state::SoulCampfire::MIN_STATE_ID,
            state::SoulCampfire::MAX_STATE_ID,
        ),
        range(blocks::MAGMA_BLOCK, blocks::MAGMA_BLOCK),
        range(
            state::SweetBerryBush::MIN_STATE_ID,
            state::SweetBerryBush::MAX_STATE_ID,
        ),
        range(blocks::WITHER_ROSE, blocks::WITHER_ROSE),
        range(state::Cactus::MIN_STATE_ID, state::Cactus::MAX_STATE_ID),
        range(blocks::POWDER_SNOW, blocks::POWDER_SNOW),
        range(blocks::COBWEB, blocks::COBWEB),
    ];
    hazards
        .iter()
        .any(|r| r.contains(&id))
        .then_some(CellKind::Hazard)
}

pub fn classify(model: BlockModel, id: i32) -> Cell {
    let kind = special_kind(id);
    match model {
        BlockModel::FullBlocks => full_block_cell(id, kind),
        BlockModel::Vanilla => shape_cell(id, kind),
    }
}

fn full_block_cell(id: i32, kind: Option<CellKind>) -> Cell {
    if !is_solid_block_state(id) {
        return Cell::EMPTY.with_kind(kind.unwrap_or(CellKind::Normal));
    }
    let kind = match kind {
        Some(CellKind::Lava) => CellKind::Hazard,
        Some(CellKind::Water) | None => CellKind::Normal,
        Some(other) => other,
    };
    Cell::FULL.with_kind(kind)
}

const CENTER_MIN: f32 = 0.25;
const CENTER_MAX: f32 = 0.75;
const THIN: f32 = 0.25;
const EPSILON: f32 = 1.0e-4;

fn shape_cell(id: i32, kind: Option<CellKind>) -> Cell {
    let boxes = shapes::for_state(id);
    let mut bottom = 16.0f32;
    let mut top = 0.0f32;
    let mut edges = 0u8;
    let mut full_low_slab = false;
    for b in boxes {
        let covers_center =
            b.x0 < CENTER_MAX && b.x1 > CENTER_MIN && b.z0 < CENTER_MAX && b.z1 > CENTER_MIN;
        if covers_center {
            bottom = bottom.min(b.y0);
            top = top.max(b.y1);
            if b.x0 <= EPSILON && b.z0 <= EPSILON && b.x1 >= 1.0 - EPSILON && b.z1 >= 1.0 - EPSILON
            {
                full_low_slab |= b.y0 <= EPSILON && (b.y1 - 0.5).abs() <= EPSILON;
            }
            continue;
        }
        if b.y1 - b.y0 < 0.5 {
            continue;
        }
        let along_x = b.x1 - b.x0 >= 0.5;
        let along_z = b.z1 - b.z0 >= 0.5;
        if along_x && b.z0 <= EPSILON && b.z1 <= THIN {
            edges |= Side::North.mask();
        }
        if along_x && b.z1 >= 1.0 - EPSILON && b.z0 >= 1.0 - THIN {
            edges |= Side::South.mask();
        }
        if along_z && b.x0 <= EPSILON && b.x1 <= THIN {
            edges |= Side::West.mask();
        }
        if along_z && b.x1 >= 1.0 - EPSILON && b.x0 >= 1.0 - THIN {
            edges |= Side::East.mask();
        }
    }
    let mut cell = if top > bottom {
        let top = ((top * 16.0).ceil() as i32).clamp(1, MAX_TOP as i32) as u8;
        let bottom = ((bottom * 16.0).floor() as i32).clamp(0, 15) as u8;
        Cell::solid(bottom.min(top - 1), top)
    } else {
        Cell::EMPTY
    };
    cell = cell.with_edges(edges);
    let stair = full_low_slab && cell.top() >= 16;
    match (kind, stair) {
        (Some(kind), _) => cell.with_kind(kind),
        (None, true) => cell.with_kind(CellKind::Stair),
        (None, false) => cell,
    }
}

#[cfg(test)]
mod tests {
    use voidmc_data::v26_1_2::props::{Facing4, Half2Upper, Hinge};
    use voidmc_data::v26_1_2::state::OakDoor;

    use super::*;

    fn vanilla(id: i32) -> Cell {
        classify(BlockModel::Vanilla, id)
    }

    #[test]
    fn table_matches_live_classification() {
        let table = CellTable::new(BlockModel::Vanilla);
        for state in [
            blocks::AIR,
            blocks::OAK_STAIRS,
            blocks::FIREFLY_BUSH,
            blocks::LAVA,
        ] {
            assert_eq!(table.get(state), vanilla(state));
        }
        assert_eq!(table.get(-1), Cell::FULL);
        assert_eq!(table.get(1_000_000), Cell::FULL);
    }

    #[test]
    fn full_and_empty_blocks() {
        assert_eq!(vanilla(blocks::STONE), Cell::FULL);
        assert_eq!(vanilla(blocks::AIR), Cell::EMPTY);
        assert!(!vanilla(blocks::SHORT_GRASS).has_collision());
    }

    #[test]
    fn slabs_carpets_and_fences_keep_their_heights() {
        let slab = vanilla(blocks::OAK_SLAB);
        assert_eq!((slab.bottom(), slab.top()), (0, 8));
        let carpet = vanilla(blocks::WHITE_CARPET);
        assert_eq!((carpet.bottom(), carpet.top()), (0, 1));
        assert_eq!(vanilla(blocks::OAK_FENCE).top(), 24);
        assert_eq!(vanilla(blocks::COBBLESTONE_WALL).top(), 24);
    }

    #[test]
    fn stairs_are_marked_as_half_steps() {
        let stairs = vanilla(blocks::OAK_STAIRS);
        assert_eq!(stairs.kind(), CellKind::Stair);
        assert_eq!(stairs.top(), 16);
    }

    #[test]
    fn doors_are_thin_edges_not_solid_cells() {
        let closed = OakDoor {
            facing: Facing4::North,
            half: Half2Upper::Lower,
            hinge: Hinge::Left,
            open: false,
            powered: false,
        };
        let open = OakDoor {
            open: true,
            ..closed
        };
        let closed = vanilla(closed.to_state_id());
        let open = vanilla(open.to_state_id());
        assert!(!closed.has_collision());
        assert!(!open.has_collision());
        assert_ne!(closed.edges(), 0);
        assert_ne!(open.edges(), 0);
        assert_ne!(closed.edges(), open.edges());
    }

    #[test]
    fn fluids_and_hazards_are_tagged() {
        assert_eq!(vanilla(blocks::WATER).kind(), CellKind::Water);
        assert!(!vanilla(blocks::WATER).has_collision());
        assert_eq!(vanilla(blocks::LAVA).kind(), CellKind::Lava);
        assert_eq!(vanilla(blocks::MAGMA_BLOCK).kind(), CellKind::Hazard);
        assert!(vanilla(blocks::MAGMA_BLOCK).has_collision());
        assert_eq!(vanilla(blocks::FIRE).kind(), CellKind::Hazard);
        assert_eq!(vanilla(blocks::CACTUS).kind(), CellKind::Hazard);
    }

    #[test]
    fn full_block_model_matches_entity_physics() {
        let model = BlockModel::FullBlocks;
        assert_eq!(classify(model, blocks::OAK_SLAB), Cell::FULL);
        assert_eq!(classify(model, blocks::SHORT_GRASS), Cell::FULL);
        assert_eq!(classify(model, blocks::AIR), Cell::EMPTY);
        assert_eq!(classify(model, blocks::WATER).kind(), CellKind::Water);
        assert!(!classify(model, blocks::WATER).has_collision());
        let lava = classify(model, blocks::LAVA);
        assert!(lava.has_collision());
        assert_eq!(lava.kind(), CellKind::Hazard);
    }
}
