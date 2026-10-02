use super::cell::{Cell, CellKind, Side};
use super::grid::NavWorld;
use super::math::BlockPos;
use super::profile::{Mobility, MoveModel};

pub const SQRT2: f32 = std::f32::consts::SQRT_2;
const SQRT3: f32 = 1.732_050_8;

/// Where a body can rest: the node's feet height in absolute sixteenths and
/// the cost multiplier of the terrain it occupies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stand {
    pub floor: i32,
    pub cost: f32,
    pub stair: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Move {
    pub pos: BlockPos,
    pub floor: i32,
    pub cost: f32,
}

#[derive(Clone, Copy, Default)]
struct Terrain {
    water: bool,
    hazard: bool,
    lava: bool,
    edges: u8,
}

impl Terrain {
    #[inline]
    fn absorb(&mut self, cell: Cell) {
        self.edges |= cell.edges();
        match cell.kind() {
            CellKind::Water => self.water = true,
            CellKind::Hazard => self.hazard = true,
            CellKind::Lava => self.lava = true,
            _ => {}
        }
    }

    fn cost(self, model: &MoveModel) -> Option<f32> {
        let mut cost = 1.0f32;
        if self.lava {
            cost = cost.max(model.lava?);
        }
        if self.hazard {
            cost = cost.max(model.hazard?);
        }
        if self.water {
            cost = cost.max(model.water?);
        }
        Some(cost)
    }
}

#[inline]
fn cell_base(y: i32) -> i32 {
    y * 16
}

/// Scans one column for anything overlapping the body span `[lo, hi)`.
/// The cell below the span is checked too, since fences and walls reach 8
/// sixteenths into the block above them.
#[inline]
fn scan_column(
    world: &mut impl NavWorld,
    x: i32,
    z: i32,
    lo: i32,
    hi: i32,
    terrain: &mut Terrain,
) -> bool {
    let first = lo >> 4;
    let below = world.cell(BlockPos::new(x, first - 1, z));
    if below.top() > 16 && below.overlaps(cell_base(first - 1), lo, hi) {
        return false;
    }
    let mut y = first;
    while cell_base(y) < hi {
        let cell = world.cell(BlockPos::new(x, y, z));
        if cell.is_unloaded() || cell.overlaps(cell_base(y), lo, hi) {
            return false;
        }
        terrain.absorb(cell);
        y += 1;
    }
    true
}

#[inline]
fn body_clear(
    model: &MoveModel,
    world: &mut impl NavWorld,
    x: i32,
    z: i32,
    lo: i32,
    hi: i32,
    terrain: &mut Terrain,
) -> bool {
    if lo >= hi {
        return true;
    }
    let r = model.radius;
    for cz in z - r..=z + r {
        for cx in x - r..=x + r {
            if !scan_column(world, cx, cz, lo, hi, terrain) {
                return false;
            }
        }
    }
    true
}

/// The feet height a walker would rest at with its feet inside block `pos`:
/// on a partial block in that cell (slab, carpet) or on a full/tall block
/// below it, with clearance for the whole body.
pub fn stand(model: &MoveModel, world: &mut impl NavWorld, pos: BlockPos) -> Option<Stand> {
    match model.mobility {
        Mobility::Walk => stand_walk(model, world, pos),
        Mobility::Fly => stand_free(model, world, pos, false),
        Mobility::Swim => stand_free(model, world, pos, true),
    }
}

fn stand_walk(model: &MoveModel, world: &mut impl NavWorld, pos: BlockPos) -> Option<Stand> {
    let cell = world.cell(pos);
    if cell.is_unloaded() {
        return None;
    }
    let (floor, support) = if cell.has_collision() {
        if cell.bottom() != 0 || cell.top() >= 16 {
            return None;
        }
        (cell_base(pos.y) + cell.top(), cell)
    } else {
        let below = world.cell(pos.offset(0, -1, 0));
        if below.top() < 16 {
            return None;
        }
        (cell_base(pos.y - 1) + below.top(), below)
    };
    let mut terrain = Terrain::default();
    if support.kind() == CellKind::Hazard {
        terrain.hazard = true;
    }
    if !body_clear(
        model,
        world,
        pos.x,
        pos.z,
        floor,
        floor + model.head,
        &mut terrain,
    ) {
        return None;
    }
    Some(Stand {
        floor,
        cost: terrain.cost(model)?,
        stair: support.kind() == CellKind::Stair,
    })
}

fn stand_free(
    model: &MoveModel,
    world: &mut impl NavWorld,
    pos: BlockPos,
    water_only: bool,
) -> Option<Stand> {
    let floor = cell_base(pos.y);
    if water_only && world.cell(pos).kind() != CellKind::Water {
        return None;
    }
    let mut terrain = Terrain::default();
    if !body_clear(
        model,
        world,
        pos.x,
        pos.z,
        floor,
        floor + model.head,
        &mut terrain,
    ) {
        return None;
    }
    if water_only {
        terrain.water = false;
    }
    Some(Stand {
        floor,
        cost: terrain.cost(model)?,
        stair: false,
    })
}

const DIRS8: [(i32, i32); 8] = [
    (0, -1),
    (1, 0),
    (0, 1),
    (-1, 0),
    (1, -1),
    (1, 1),
    (-1, 1),
    (-1, -1),
];

const COLUMN_SPAN: i32 = 16;

/// The cells of one column read during a single expansion, so the walk
/// model touches each block at most once per expansion.
struct Column {
    x: i32,
    z: i32,
    base: i32,
    known: u16,
    cells: [Cell; COLUMN_SPAN as usize],
}

impl Column {
    fn new(x: i32, z: i32, base: i32) -> Self {
        Self {
            x,
            z,
            base,
            known: 0,
            cells: [Cell::EMPTY; COLUMN_SPAN as usize],
        }
    }

    #[inline]
    fn get(&mut self, world: &mut impl NavWorld, y: i32) -> Cell {
        let slot = y - self.base;
        if !(0..COLUMN_SPAN).contains(&slot) {
            return world.cell(BlockPos::new(self.x, y, self.z));
        }
        let bit = 1u16 << slot;
        if self.known & bit == 0 {
            self.cells[slot as usize] = world.cell(BlockPos::new(self.x, y, self.z));
            self.known |= bit;
        }
        self.cells[slot as usize]
    }

    fn clear(
        &mut self,
        world: &mut impl NavWorld,
        lo: i32,
        hi: i32,
        terrain: &mut Terrain,
    ) -> bool {
        if lo >= hi {
            return true;
        }
        let first = lo >> 4;
        let below = self.get(world, first - 1);
        if below.top() > 16 && below.overlaps(cell_base(first - 1), lo, hi) {
            return false;
        }
        let mut y = first;
        while cell_base(y) < hi {
            let cell = self.get(world, y);
            if cell.is_unloaded() || cell.overlaps(cell_base(y), lo, hi) {
                return false;
            }
            terrain.absorb(cell);
            y += 1;
        }
        true
    }
}

#[inline]
fn footprint_clear(
    model: &MoveModel,
    world: &mut impl NavWorld,
    column: &mut Column,
    lo: i32,
    hi: i32,
    terrain: &mut Terrain,
) -> bool {
    if !column.clear(world, lo, hi, terrain) {
        return false;
    }
    if model.radius == 0 || lo >= hi {
        return true;
    }
    let (x, z, r) = (column.x, column.z, model.radius);
    for cz in z - r..=z + r {
        for cx in x - r..=x + r {
            if (cx != x || cz != z) && !scan_column(world, cx, cz, lo, hi, terrain) {
                return false;
            }
        }
    }
    true
}

#[derive(Clone, Copy)]
struct Reached {
    step: Move,
    edges: u8,
}

/// Every node reachable in one move from `from` (feet at `floor`), appended to
/// `out` with its cost.
pub fn neighbors(
    model: &MoveModel,
    world: &mut impl NavWorld,
    from: BlockPos,
    floor: i32,
    out: &mut Vec<Move>,
) {
    match model.mobility {
        Mobility::Walk => walk_neighbors(model, world, from, floor, out),
        Mobility::Fly | Mobility::Swim => free_neighbors(model, world, from, out),
    }
}

fn walk_neighbors(
    model: &MoveModel,
    world: &mut impl NavWorld,
    from: BlockPos,
    floor: i32,
    out: &mut Vec<Move>,
) {
    let base = ((floor - model.max_fall) >> 4) - 2;
    let mut origin = Column::new(from.x, from.z, base);
    let mut origin_terrain = Terrain::default();
    origin.clear(world, floor, floor + model.head, &mut origin_terrain);
    let origin_edges = origin_terrain.edges;

    let mut sides: [Option<(Column, Reached)>; 4] = [None, None, None, None];
    for (index, &(dx, dz)) in DIRS8.iter().enumerate().take(4) {
        let mut target = Column::new(from.x + dx, from.z + dz, base);
        if let Some(reached) = step_to(
            model,
            world,
            &mut origin,
            origin_edges,
            &mut target,
            floor,
            Side::of_step(dx, dz),
        ) {
            out.push(reached.step);
            sides[index] = Some((target, reached));
        }
    }
    for &(dx, dz) in &DIRS8[4..] {
        let along_x = if dx > 0 { 1 } else { 3 };
        let along_z = if dz > 0 { 2 } else { 0 };
        let (Some(_), Some(_)) = (&sides[along_x], &sides[along_z]) else {
            continue;
        };
        let mut target = Column::new(from.x + dx, from.z + dz, base);
        let Some(reached) = step_to(
            model,
            world,
            &mut origin,
            origin_edges,
            &mut target,
            floor,
            None,
        ) else {
            continue;
        };
        let mut edges = origin_edges | reached.edges;
        let lo = floor.min(reached.step.floor);
        let hi = floor.max(reached.step.floor) + model.head;
        let mut clear = true;
        for side in [along_x, along_z] {
            let Some((column, side_reached)) = sides[side].as_mut() else {
                clear = false;
                break;
            };
            edges |= side_reached.edges;
            let flat = side_reached.step.floor == floor && reached.step.floor == floor;
            let mut terrain = Terrain::default();
            if !flat && !footprint_clear(model, world, column, lo, hi, &mut terrain) {
                clear = false;
                break;
            }
            edges |= terrain.edges;
        }
        if clear && edges == 0 {
            out.push(reached.step);
        }
    }
}

fn step_to(
    model: &MoveModel,
    world: &mut impl NavWorld,
    origin: &mut Column,
    origin_edges: u8,
    target: &mut Column,
    floor: i32,
    side: Option<Side>,
) -> Option<Reached> {
    let top_cell = (floor + model.rise) >> 4;
    let bottom_cell = (floor - model.max_fall) >> 4;
    let mut ny = top_cell;
    while ny >= bottom_cell {
        let cell = target.get(world, ny);
        if cell.is_unloaded() {
            return None;
        }
        let candidate = if cell.has_collision() {
            (cell.bottom() == 0 && cell.top() < 16).then(|| (cell_base(ny) + cell.top(), cell))
        } else {
            let below = target.get(world, ny - 1);
            (below.top() >= 16).then(|| (cell_base(ny - 1) + below.top(), below))
        };
        let Some((stand_floor, support)) = candidate else {
            ny -= 1;
            continue;
        };
        let rise = stand_floor - floor;
        let stair = support.kind() == CellKind::Stair;
        let effective_rise = if stair { rise.min(8) } else { rise };
        if effective_rise > model.rise {
            ny -= 1;
            continue;
        }
        if -rise > model.max_fall {
            return None;
        }
        let mut terrain = Terrain::default();
        if support.kind() == CellKind::Hazard {
            terrain.hazard = true;
        }
        if !footprint_clear(
            model,
            world,
            target,
            stand_floor,
            stand_floor + model.head,
            &mut terrain,
        ) {
            return None;
        }
        let passage = if rise > 0 {
            let mut headroom = Terrain::default();
            let clear = footprint_clear(
                model,
                world,
                origin,
                floor + model.head,
                stand_floor + model.head,
                &mut headroom,
            );
            terrain.edges |= headroom.edges;
            clear
        } else {
            footprint_clear(
                model,
                world,
                target,
                stand_floor + model.head,
                floor + model.head,
                &mut terrain,
            )
        };
        if !passage {
            return None;
        }
        let terrain_cost = terrain.cost(model)?;
        if let Some(side) = side
            && (origin_edges | terrain.edges) != 0
        {
            let lo = floor.max(stand_floor);
            let hi = lo + model.head;
            let mut y = lo >> 4;
            while cell_base(y) < hi {
                if origin.get(world, y).blocks_side(side)
                    || target.get(world, y).blocks_side(side.opposite())
                {
                    return None;
                }
                y += 1;
            }
        }
        let diagonal = side.is_none();
        let mut cost = if diagonal { SQRT2 } else { 1.0 } * terrain_cost;
        if effective_rise > model.step {
            cost += model.jump_cost;
        }
        if rise < 0 {
            cost += model.fall_cost * (-rise) as f32 / 16.0;
        }
        return Some(Reached {
            step: Move {
                pos: BlockPos::new(target.x, ny, target.z),
                floor: stand_floor,
                cost,
            },
            edges: terrain.edges,
        });
    }
    None
}

fn edge_blocked(
    world: &mut impl NavWorld,
    from: BlockPos,
    to: BlockPos,
    side: Side,
    lo: i32,
    hi: i32,
) -> bool {
    let mut y = lo >> 4;
    while cell_base(y) < hi {
        if world
            .cell(BlockPos::new(from.x, y, from.z))
            .blocks_side(side)
            || world
                .cell(BlockPos::new(to.x, y, to.z))
                .blocks_side(side.opposite())
        {
            return true;
        }
        y += 1;
    }
    false
}

fn free_neighbors(
    model: &MoveModel,
    world: &mut impl NavWorld,
    from: BlockPos,
    out: &mut Vec<Move>,
) {
    let mut open = [None::<Stand>; 27];
    let slot = |dx: i32, dy: i32, dz: i32| ((dy + 1) * 9 + (dz + 1) * 3 + (dx + 1)) as usize;
    for axes in 1..=3 {
        for dy in -1..=1i32 {
            for dz in -1..=1i32 {
                for dx in -1..=1i32 {
                    if dx.abs() + dy.abs() + dz.abs() != axes {
                        continue;
                    }
                    if axes > 1 {
                        let components = [
                            (dx, 0, 0),
                            (0, dy, 0),
                            (0, 0, dz),
                            (dx, dy, 0),
                            (dx, 0, dz),
                            (0, dy, dz),
                        ];
                        let cut = components.iter().any(|&(cx, cy, cz)| {
                            let n = cx.abs() + cy.abs() + cz.abs();
                            n > 0 && n < axes && open[slot(cx, cy, cz)].is_none()
                        });
                        if cut {
                            continue;
                        }
                    }
                    let target = from.offset(dx, dy, dz);
                    let Some(stand) =
                        stand_free(model, world, target, model.mobility == Mobility::Swim)
                    else {
                        continue;
                    };
                    if axes == 1
                        && let Some(side) = Side::of_step(dx, dz)
                        && edge_blocked(
                            world,
                            from,
                            target,
                            side,
                            stand.floor,
                            stand.floor + model.head,
                        )
                    {
                        continue;
                    }
                    open[slot(dx, dy, dz)] = Some(stand);
                    let base = match axes {
                        1 => 1.0,
                        2 => SQRT2,
                        _ => SQRT3,
                    };
                    out.push(Move {
                        pos: target,
                        floor: stand.floor,
                        cost: base * stand.cost,
                    });
                }
            }
        }
    }
}

/// Admissible distance estimate: octile on the ground plane for walkers (a
/// height change always rides on a horizontal move), 3D octile otherwise.
#[inline]
pub fn heuristic(model: &MoveModel, from: BlockPos, to: BlockPos) -> f32 {
    let dx = (from.x - to.x).unsigned_abs() as f32;
    let dz = (from.z - to.z).unsigned_abs() as f32;
    match model.mobility {
        Mobility::Walk => {
            let (lo, hi) = if dx < dz { (dx, dz) } else { (dz, dx) };
            hi + (SQRT2 - 1.0) * lo
        }
        Mobility::Fly | Mobility::Swim => {
            let dy = (from.y - to.y).unsigned_abs() as f32;
            let mut d = [dx, dy, dz];
            d.sort_unstable_by(f32::total_cmp);
            d[2] + (SQRT2 - 1.0) * d[1] + (SQRT3 - SQRT2) * d[0]
        }
    }
}
