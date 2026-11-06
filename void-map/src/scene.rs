//! The block model of the world: a floating island, drawn rather than measured.
//!
//! The geography is the generator's. Its land and sea stay where they are, near
//! enough, but its waves are bent and smoothed, because a regular world reads as a
//! chequerboard at this size.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::num::{NonZeroU8, NonZeroU16};

use voidmc::BiomeId;
use voidmc_codec::VarI64;
use voidmc_data::v26_1_2::blocks as b;
use voidmc_protocol::clientbound::{Chunk, ChunkBuilder, LightData, SectionBlocksUpdate};

use crate::buildings::landmark;
use crate::markers::MarkerKind;
use crate::noise::{fractal, scatter};
use crate::regions::{Region, RegionState, Regions, ZoneLook, faded};
use crate::terrain::Terrain;
use crate::window::Window;

/// Height of the sea on the model.
pub const SEA_Y: i32 = 64;
/// The world the map covers, in blocks from the origin.
pub const WORLD_RADIUS: f64 = 464.0;
const LOWEST_Y: i32 = 24;

/// How much of the world a model block stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scale {
    /// Widest radius of the model, in blocks.
    pub radius: i32,
    /// World blocks per model block.
    pub blocks_per_cell: u8,
    /// Model blocks a terrace rises by.
    pub rise: i32,
    /// Model blocks per block of a landmark.
    pub build: i32,
    /// Highest block allowed.
    pub max_y: i32,
}

/// The whole map on one screen.
pub const OVERVIEW: Scale = Scale {
    radius: 58,
    blocks_per_cell: 8,
    rise: 2,
    build: 1,
    max_y: 88,
};

/// Between the two others: each step of a zoom makes blocks less than twice as large.
pub const MIDDLE: Scale = Scale {
    radius: 93,
    blocks_per_cell: 5,
    rise: 2,
    build: 1,
    max_y: 100,
};

/// The same map, a little under three times wider than the overview.
pub const DETAIL: Scale = Scale {
    radius: 155,
    blocks_per_cell: 3,
    rise: 3,
    build: 2,
    max_y: 120,
};

/// World blocks of elevation per terrace.
const TERRACE_STEP: i32 = 3;
const FOREST_LINE: i32 = 2;
const ROCK_TERRACE: i32 = 5;
const SNOW_TERRACE: i32 = 8;
const BEACH_LINE: i32 = 1;
const SHALLOWS: i32 = 3;

/// All in world blocks, so that both scales draw the same island.
const WARP_SIZE: f64 = 150.0;
const WARP: f64 = 90.0;
const SMOOTHING: f64 = 16.0;
const RELIEF_SIZE: f64 = 120.0;
const RELIEF: f64 = 7.0;
const RIM_SIZE: f64 = 170.0;
const RIM_WANDER: f64 = 0.14;
const SHORE_WIDTH: f64 = 22.0;
const FOREST_SIZE: f64 = 90.0;
const FOREST_DENSITY: f64 = 0.02;
const CANOPY_SIZE: f64 = 22.0;

const WARP_X_SEED: u32 = 10;
const WARP_Z_SEED: u32 = 20;
const RELIEF_SEED: u32 = 30;
const RIM_SEED: u32 = 40;
const FOREST_SEED: u32 = 60;
const CANOPY_SEED: u32 = 70;
const PATH_SEED: u32 = 80;

/// What a model is built of, for a player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Blocks {
    /// Flat colours, as on a printed map.
    #[default]
    Flat,
    /// The blocks of the world, with their textures.
    Real,
}

impl Blocks {
    pub const ALL: [Self; 2] = [Self::Flat, Self::Real];

    pub fn label(self) -> &'static str {
        match self {
            Self::Flat => "Couleurs plates",
            Self::Real => "Vrais blocs",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Flat => Self::Real,
            Self::Real => Self::Flat,
        }
    }
}

/// Everything a player can change about the chunks of a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ChunkLook {
    pub zone: ZoneLook,
    pub blocks: Blocks,
}

/// The blocks of one section of a chunk that have to change, as they go on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionChange {
    /// Its height, in sections.
    pub section: i32,
    pub blocks: Vec<VarI64>,
}

/// The block of the world that a colour of the palette stands for.
fn real_block(block: i32) -> i32 {
    match block {
        GRASS | FOREST => b::GRASS_BLOCK,
        CANOPY => b::OAK_LEAVES,
        ROCK => b::STONE,
        SNOW => b::SNOW_BLOCK,
        SAND => b::SAND,
        SOIL => b::DIRT,
        PATH => b::DIRT_PATH,
        SHALLOW_WATER | DEEP_WATER => b::WATER,
        other => other,
    }
}

const GRASS: i32 = b::LIME_CONCRETE;
const FOREST: i32 = b::GREEN_CONCRETE;
const CANOPY: i32 = b::LIME_TERRACOTTA;
const ROCK: i32 = b::LIGHT_GRAY_CONCRETE;
const SNOW: i32 = b::WHITE_CONCRETE;
const SAND: i32 = b::SMOOTH_SANDSTONE;
const SOIL: i32 = b::BROWN_TERRACOTTA;
const PATH: i32 = b::BROWN_CONCRETE;
const BRIDGE: i32 = b::SPRUCE_PLANKS;
const CLOUD_SEED: u32 = 90;
/// What a cloud is made of, wherever it hangs. The faces of a block take care of its shade.
const CLOUD: i32 = b::WHITE_CONCRETE;
/// Each cell of a grid this wide may hold one cloud, in world blocks: clouds are the same
/// on every model.
const CLOUD_SPACING: f64 = 84.0;
/// The share of the cells that do.
const CLOUD_CHANCE: f64 = 0.8;
/// A pixel of a cloud, in world blocks: its edge stays that of a drawing on the close
/// model. A coarser one draws it cell by cell.
const CLOUD_PIXEL: f64 = 6.0;
/// The length of a cloud, along its axis, in world blocks.
const CLOUD_LENGTH: [f64; 2] = [30.0, 118.0];
/// The radius of its puffs.
const CLOUD_PUFF: [f64; 2] = [11.0, 25.0];
/// How far a cloud turns from its west-east axis, as a share of a turn.
const CLOUD_TILT: f64 = 0.09;
/// How high in a puff each layer of a cloud starts: 0 at its edge, 1 at its middle.
const CLOUD_LAYERS: [f64; 4] = [0.0, 0.34, 0.58, 0.78];
/// How far south of its place a cloud is drawn, in world blocks. The camera looks north
/// and down, so that a cloud high up shows further north than what it hangs over.
const CLOUD_LEAN: f64 = 48.0;
/// Over the land under it, in blocks of a landmark: more than the tallest one.
const CLOUD_CLEARANCE: i32 = 8;
/// The most a cloud hangs over that, in layers: no two hang alike.
const CLOUD_DRIFT: f64 = 4.0;
/// The highest a cloud starts, as a share of the model's height over the sea: a camera
/// zoomed all the way in stays over it.
const CLOUD_HEIGHT: f64 = 0.62;
/// How far from its real place a figure is shown on a coarser model, in cells.
const STAND_REACH: i32 = 4;
const HATCH_WIDTH: i32 = 3;
const PALISADE_HEIGHT: i32 = 3;
const SHALLOW_WATER: i32 = b::CYAN_CONCRETE;
const DEEP_WATER: i32 = b::LIGHT_BLUE_CONCRETE;

/// Blocks under the surface where cliffs turn dark, then black.
const SOIL_DEPTH: i32 = 2;
const DARK_DEPTH: i32 = 10;
const BLACK_DEPTH: i32 = 16;
const UNDERSIDE_DEPTH: i32 = 34;
const UNDERSIDE_SLOPE: f64 = 1.5;

impl Scale {
    fn size(self) -> u16 {
        self.radius as u16 * 2
    }

    /// The world window the model shows: all of it, centred on the origin.
    pub fn window(self) -> Window {
        Window::new(
            0,
            0,
            NonZeroU16::new(self.size()).unwrap_or(NonZeroU16::MIN),
            NonZeroU16::new(self.size()).unwrap_or(NonZeroU16::MIN),
            NonZeroU8::new(self.blocks_per_cell).unwrap_or(NonZeroU8::MIN),
        )
    }

    /// The world block at the middle of a model cell.
    fn world(self, cell: i32) -> f64 {
        (f64::from(cell) + 0.5) * f64::from(self.blocks_per_cell)
    }

    fn cells(self, world: f64) -> f64 {
        world / f64::from(self.blocks_per_cell)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Column {
    /// Highest solid block.
    ground: i32,
    surface: i32,
    /// Lowest solid block.
    bottom: i32,
    /// What covers the ground up to the sea.
    water: i32,
    state: RegionState,
    /// Next to a region in another state.
    border: bool,
}

impl Column {
    /// Highest block, solid or sea.
    fn top(self) -> i32 {
        self.ground.max(SEA_Y)
    }

    fn is_land(self) -> bool {
        self.ground >= SEA_Y
    }
}

/// Columns of a block model. Coordinates are in blocks from its centre.
pub struct Scene {
    scale: Scale,
    columns: Vec<Option<Column>>,
    buildings: HashMap<(i32, i32), Vec<(i32, i32)>>,
    /// The heights of the clouds over each column, worked out once, on and off the land.
    clouds: HashMap<(i32, i32), Vec<i32>>,
}

/// One cloud: a few round puffs along an axis, the same on every model, at a place of the
/// world. Each is its own shape, which comes from where it hangs.
#[derive(Debug, Clone, PartialEq)]
struct Cloud {
    /// Their middles and radii, in world blocks.
    puffs: Vec<[f64; 3]>,
    /// How many layers it has at its middle.
    layers: usize,
    /// How many layers it hangs over the lowest clouds.
    drift: i32,
}

impl Cloud {
    /// The cloud over a place, drawn from four rolls per step of the shape.
    fn grown(middle: [f64; 2], cell: (i32, i32)) -> Self {
        let roll = |step: u32| scatter(CLOUD_SEED + 10 * step, cell.0, cell.1);
        let between = |[low, high]: [f64; 2], share: f64| low + (high - low) * share;
        let [length, tilt, layers, drift] = roll(1);
        // Small clouds are the common ones.
        let length = between(CLOUD_LENGTH, length * length);
        let tilt = (tilt * 2.0 - 1.0) * CLOUD_TILT * std::f64::consts::TAU;
        let axis = [tilt.cos(), tilt.sin()];
        let count = (length / CLOUD_PUFF[0] / 1.6).ceil().max(2.0) as u32;
        let puffs = (0..count)
            .map(|index| {
                let [along, across, size, _] = roll(2 + index);
                let along = (f64::from(index) + 0.2 + along * 0.6) / f64::from(count) - 0.5;
                let across = (across * 2.0 - 1.0) * CLOUD_PUFF[0];
                // The ends of a cloud are thinner than its middle.
                let belly = 1.0 - along.abs() * 1.2;
                [
                    middle[0] + axis[0] * along * length - axis[1] * across,
                    middle[1] + axis[1] * along * length + axis[0] * across,
                    between(CLOUD_PUFF, size) * belly,
                ]
            })
            .collect();
        Self {
            puffs,
            layers: 1 + (layers * (length / CLOUD_LENGTH[1] * 3.0 + 1.0)) as usize,
            drift: (drift * CLOUD_DRIFT) as i32,
        }
    }

    /// Its bounds, as west, north, east and south, in world blocks.
    fn bounds(&self) -> [f64; 4] {
        self.puffs.iter().fold(
            [f64::MAX, f64::MAX, f64::MIN, f64::MIN],
            |[west, north, east, south], [x, z, radius]| {
                [
                    west.min(x - radius),
                    north.min(z - radius),
                    east.max(x + radius),
                    south.max(z + radius),
                ]
            },
        )
    }

    /// How many layers stand at a world block.
    fn layers_at(&self, x: f64, z: f64) -> usize {
        let height = self
            .puffs
            .iter()
            .map(|[puff_x, puff_z, radius]| 1.0 - (puff_x - x).hypot(puff_z - z) / radius)
            .fold(f64::MIN, f64::max);
        let reached = CLOUD_LAYERS.iter().filter(|start| height > **start);
        reached.count().min(self.layers)
    }

    fn overlaps(&self, other: &Self) -> bool {
        let ([west, north, east, south], [other_west, other_north, other_east, other_south]) =
            (self.bounds(), other.bounds());
        west < other_east && other_west < east && north < other_south && other_north < south
    }
}

/// The clouds of the closed regions. They depend on the world alone, so a cloud stays
/// where it is from one model to the next.
fn cloud_list(regions: &Regions) -> Vec<Cloud> {
    let cells = (WORLD_RADIUS / CLOUD_SPACING).ceil() as i32;
    let mut clouds: Vec<Cloud> = Vec::new();
    for cell_z in -cells..=cells {
        for cell_x in -cells..=cells {
            let [jitter_x, jitter_z, chance, _] = scatter(CLOUD_SEED, cell_x, cell_z);
            let at = [
                (f64::from(cell_x) + 0.2 + jitter_x * 0.6) * CLOUD_SPACING,
                (f64::from(cell_z) + 0.2 + jitter_z * 0.6) * CLOUD_SPACING,
            ];
            if regions.at(at[0], at[1]).state == RegionState::Discovered
                || chance >= CLOUD_CHANCE
                || at[0].hypot(at[1]) > rim_radius(at[0], at[1])
            {
                continue;
            }
            let cloud = Cloud::grown([at[0], at[1] + CLOUD_LEAN], (cell_x, cell_z));
            let [west, north, east, south] = cloud.bounds();
            let inside = |low: f64, high: f64| low >= -WORLD_RADIUS && high <= WORLD_RADIUS;
            if inside(west, east)
                && inside(north, south)
                && !clouds.iter().any(|other| cloud.overlaps(other))
            {
                clouds.push(cloud);
            }
        }
    }
    clouds
}

/// World blocks from the origin to the rim, around a place.
fn rim_radius(x: f64, z: f64) -> f64 {
    WORLD_RADIUS * (0.9 + fractal(RIM_SEED, x, z, RIM_SIZE) * RIM_WANDER - RIM_WANDER / 2.0)
}

fn surface_block(biome: BiomeId, elevation: i32, terrace: i32) -> i32 {
    let is = |names: &[&str]| names.iter().any(|name| BiomeId::named(name) == Some(biome));
    if terrace >= SNOW_TERRACE || is(&["snowy_plains", "snowy_taiga", "ice_spikes"]) {
        SNOW
    } else if terrace >= ROCK_TERRACE || is(&["stony_shore"]) {
        ROCK
    } else if elevation <= BEACH_LINE || is(&["desert", "beach"]) {
        SAND
    } else if is(&["badlands"]) {
        SOIL
    } else {
        GRASS
    }
}

fn cliff_block(surface: i32, depth: i32) -> i32 {
    if depth >= BLACK_DEPTH {
        b::BLACK_CONCRETE
    } else if depth >= DARK_DEPTH {
        b::GRAY_CONCRETE
    } else if depth <= SOIL_DEPTH && matches!(surface, GRASS | FOREST | CANOPY | PATH) {
        SOIL
    } else {
        b::LIGHT_GRAY_CONCRETE
    }
}

/// Averages each cell with its neighbours along rows, then along columns.
fn smooth(values: &mut [f64], size: usize, reach: usize) {
    for stride in [(1, size), (size, 1)] {
        let source = values.to_vec();
        for line in 0..size {
            for along in 0..size {
                let (from, to) = (along.saturating_sub(reach), (along + reach).min(size - 1));
                let sum: f64 = (from..=to)
                    .map(|at| source[at * stride.0 + line * stride.1])
                    .sum();
                values[along * stride.0 + line * stride.1] = sum / (to - from + 1) as f64;
            }
        }
    }
}

impl Scene {
    /// `terrain` must come from `scale`'s window: there is one block per cell.
    pub fn new(terrain: &Terrain, sea_level: i32, scale: Scale, regions: &Regions) -> Self {
        let size = usize::from(scale.size());
        let last = f64::from(scale.size() - 1);
        let mut elevations = Vec::with_capacity(size * size);
        let mut biomes = Vec::with_capacity(size * size);
        for row in 0..scale.size() {
            for col in 0..scale.size() {
                let (x, z) = (
                    scale.world(i32::from(col) - scale.radius),
                    scale.world(i32::from(row) - scale.radius),
                );
                let bend = |seed, at: u16| {
                    let moved = scale.cells(fractal(seed, x, z, WARP_SIZE) * WARP);
                    (f64::from(at) + moved).round().clamp(0.0, last) as u16
                };
                let (from_col, from_row) = (bend(WARP_X_SEED, col), bend(WARP_Z_SEED, row));
                elevations.push(f64::from(terrain.height(from_col, from_row) - sea_level));
                biomes.push(terrain.biome(from_col, from_row));
            }
        }
        smooth(&mut elevations, size, scale.cells(SMOOTHING) as usize);

        let mut columns = Vec::with_capacity(size * size);
        for (index, smoothed) in elevations.iter().enumerate() {
            let (x, z) = (
                (index % size) as i32 - scale.radius,
                (index / size) as i32 - scale.radius,
            );
            let (world_x, world_z) = (scale.world(x), scale.world(z));
            let relief = fractal(RELIEF_SEED, world_x, world_z, RELIEF_SIZE) * RELIEF;
            let state = regions.at(world_x, world_z).state;
            columns.push(Self::column_at(
                scale,
                [world_x, world_z],
                smoothed + relief,
                biomes[index],
                state,
            ));
        }
        Self::mark_borders(&mut columns, size, scale.build as usize);
        let clouds = Self::clouds(&columns, scale, regions);
        Self {
            scale,
            columns,
            buildings: HashMap::new(),
            clouds,
        }
    }

    /// The blocks of the clouds on this model. A cloud hangs whole, past the border of
    /// its region or the rim if need be, a little over the land it covers.
    fn clouds(
        columns: &[Option<Column>],
        scale: Scale,
        regions: &Regions,
    ) -> HashMap<(i32, i32), Vec<i32>> {
        let size = i32::from(scale.size());
        let ground_at = |x: i32, z: i32| {
            let (col, row) = (x + scale.radius, z + scale.radius);
            ((0..size).contains(&col) && (0..size).contains(&row))
                .then(|| columns[(row * size + col) as usize])
                .flatten()
                .map(|column| column.ground)
        };
        let highest = SEA_Y + (f64::from(scale.max_y - SEA_Y) * CLOUD_HEIGHT) as i32;
        let mut blocks: HashMap<(i32, i32), Vec<i32>> = HashMap::new();
        for cloud in cloud_list(regions) {
            let [west, north, east, south] = cloud.bounds();
            let cell = |world: f64| scale.cells(world).floor() as i32;
            // A whole number of cells to a pixel, or the drawing would catch on the grid.
            let pixel = scale.cells(CLOUD_PIXEL).round().max(1.0) as i32;
            let drawn = |cell: i32| {
                let first = cell.div_euclid(pixel) * pixel;
                (scale.world(first) + scale.world(first + pixel - 1)) / 2.0
            };
            let cells: Vec<(i32, i32, usize)> = (cell(north)..=cell(south))
                .flat_map(|z| (cell(west)..=cell(east)).map(move |x| (x, z)))
                .map(|(x, z)| (x, z, cloud.layers_at(drawn(x), drawn(z))))
                .filter(|(_, _, layers)| *layers > 0)
                .collect();
            let land = cells
                .iter()
                .filter_map(|(x, z, _)| ground_at(*x, *z))
                .max()
                .unwrap_or(SEA_Y);
            let bottom = (land + (CLOUD_CLEARANCE + cloud.drift) * scale.build).min(highest);
            for (x, z, layers) in cells {
                let top = bottom + layers as i32 * scale.build;
                blocks.entry((x, z)).or_default().extend(bottom..top);
            }
        }
        blocks
    }

    fn mark_borders(columns: &mut [Option<Column>], size: usize, reach: usize) {
        let states: Vec<_> = columns.iter().map(|c| c.map(|c| c.state)).collect();
        for (index, column) in columns.iter_mut().enumerate() {
            let Some(column) = column else {
                continue;
            };
            let (x, z) = (index % size, index / size);
            let around = [
                (x.saturating_sub(reach), z),
                ((x + reach).min(size - 1), z),
                (x, z.saturating_sub(reach)),
                (x, (z + reach).min(size - 1)),
            ];
            column.border = around
                .iter()
                .any(|(x, z)| states[z * size + x].is_some_and(|state| state != column.state));
        }
    }

    fn column_at(
        scale: Scale,
        [x, z]: [f64; 2],
        elevation: f64,
        biome: BiomeId,
        state: RegionState,
    ) -> Option<Column> {
        let from_rim = rim_radius(x, z) - x.hypot(z);
        if from_rim < 0.0 {
            return None;
        }
        let elevation = elevation.round() as i32;
        let elevation = if from_rim < SHORE_WIDTH {
            elevation.max(BEACH_LINE)
        } else {
            elevation
        };
        let terrace = elevation.div_euclid(TERRACE_STEP);
        let mut surface = surface_block(biome, elevation, terrace);
        let mut ground = if elevation > 0 {
            SEA_Y + 1 + terrace * scale.rise
        } else {
            SEA_Y - 1
        };
        if surface == GRASS
            && elevation > FOREST_LINE
            && fractal(FOREST_SEED, x, z, FOREST_SIZE) > FOREST_DENSITY
        {
            surface = FOREST;
            if fractal(CANOPY_SEED, x, z, CANOPY_SIZE) > 0.0 {
                surface = CANOPY;
                ground += scale.build;
            }
        }
        let depth = (scale.cells(from_rim + 8.0) * UNDERSIDE_SLOPE) as i32;
        Some(Column {
            ground: ground.min(scale.max_y),
            surface,
            bottom: (SEA_Y - 2 - depth.min(UNDERSIDE_DEPTH)).max(LOWEST_Y),
            water: if elevation >= -SHALLOWS {
                SHALLOW_WATER
            } else {
                DEEP_WATER
            },
            state,
            border: false,
        })
    }

    fn index(&self, x: i32, z: i32) -> Option<usize> {
        let radius = self.scale.radius;
        let range = -radius..radius;
        (range.contains(&x) && range.contains(&z))
            .then(|| (z + radius) as usize * usize::from(self.scale.size()) + (x + radius) as usize)
    }

    fn column(&self, x: i32, z: i32) -> Option<Column> {
        self.columns[self.index(x, z)?]
    }

    fn column_mut(&mut self, x: i32, z: i32) -> Option<&mut Column> {
        let index = self.index(x, z)?;
        self.columns[index].as_mut()
    }

    pub fn scale(&self) -> Scale {
        self.scale
    }

    /// Highest block of a column, or `None` off the model.
    pub fn top(&self, x: i32, z: i32) -> Option<i32> {
        self.column(x, z).map(Column::top)
    }

    /// Whether a block rises out of a column, that a figure would stand in.
    fn in_the_way(&self, x: i32, z: i32, column: Column) -> bool {
        column.surface == CANOPY || self.buildings.contains_key(&(x, z))
    }

    /// Whether a figure may be shown here: dry and free, on land or a bridge.
    fn standable(&self, x: i32, z: i32) -> bool {
        self.column(x, z)
            .is_some_and(|column| column.is_land() && !self.in_the_way(x, z, column))
    }

    /// Whether a figure can walk here: it is free, and so is what is around it, with no
    /// rim or step too high for a stride next to it.
    pub fn walkable(&self, x: i32, z: i32) -> bool {
        let Some(here) = self.column(x, z).filter(|column| column.is_land()) else {
            return false;
        };
        (-1..=1)
            .flat_map(|dz| (-1..=1).map(move |dx| (x + dx, z + dz)))
            .all(|(x, z)| {
                self.column(x, z).is_some_and(|side| {
                    !self.in_the_way(x, z, side)
                        && (side.top() - here.top()).abs() <= self.scale.rise
                })
            })
    }

    /// The closest cell a figure may be shown on, around a cell, and the height of the
    /// ground there. A model coarser than the one walkers go by has its shores and its
    /// trees a little elsewhere.
    pub fn stand_near(&self, x: i32, z: i32) -> Option<(i32, i32)> {
        (0..=STAND_REACH).find_map(|ring| {
            (-ring..=ring)
                .flat_map(|dz| (-ring..=ring).map(move |dx| (dx, dz)))
                .filter(|(dx, dz)| dx.abs().max(dz.abs()) == ring)
                .filter(|(dx, dz)| self.standable(x + dx, z + dz))
                .min_by_key(|(dx, dz)| dx * dx + dz * dz)
                .map(|(dx, dz)| (x + dx, z + dz))
        })
    }

    /// The model cell a world block falls on.
    /// The land of a region cell closest to the middle of that land, if it has any.
    pub fn middle_of_land(&self, regions: &Regions, region: &Region) -> Option<(i32, i32)> {
        let radius = self.scale.radius;
        let cells: Vec<_> = (-radius..radius)
            .flat_map(|x| (-radius..radius).map(move |z| (x, z)))
            .filter(|(x, z)| self.top(*x, *z).is_some_and(|top| top > SEA_Y))
            .filter(|(x, z)| {
                regions.at(self.scale.world(*x), self.scale.world(*z)).name == region.name
            })
            .collect();
        let count = i32::try_from(cells.len()).ok().filter(|count| *count > 0)?;
        let middle = cells
            .iter()
            .fold((0, 0), |sum, cell| (sum.0 + cell.0, sum.1 + cell.1));
        let middle = (middle.0 / count, middle.1 / count);
        let from_middle =
            |cell: &&(i32, i32)| (cell.0 - middle.0).pow(2) + (cell.1 - middle.1).pow(2);
        cells.iter().min_by_key(from_middle).copied()
    }

    /// How far from the middle the model's chunks go.
    pub fn chunk_radius(&self) -> i32 {
        self.scale.radius / 16 + 1
    }

    pub fn cell_of(&self, x: i32, z: i32) -> Option<(i32, i32)> {
        let (col, row) = self.scale.window().cell_at(x, z)?;
        let radius = self.scale.radius;
        Some((i32::from(col) - radius, i32::from(row) - radius))
    }

    /// Adds what tells a closed region at this column.
    fn zone_look(
        &self,
        look: ZoneLook,
        x: i32,
        z: i32,
        column: Column,
        set: &mut impl FnMut(i32, i32),
    ) {
        let build = self.scale.build;
        let locked = column.state == RegionState::Locked;
        match look {
            ZoneLook::Hatching => {
                let width = HATCH_WIDTH * build;
                if (x + z).rem_euclid(2 * width) < width {
                    let block = if locked {
                        b::RED_TERRACOTTA
                    } else {
                        b::GRAY_TERRACOTTA
                    };
                    set(column.top(), block);
                }
            }
            ZoneLook::Palisade => {
                if column.border {
                    let tall = PALISADE_HEIGHT * build + (x + z).rem_euclid(2);
                    for y in 1..=tall {
                        set(column.top() + y, b::SPRUCE_PLANKS);
                    }
                }
            }
            ZoneLook::Clouds | ZoneLook::Faded => {}
        }
    }

    /// Levels the ground around a cell, on land, and puts a landmark on it.
    pub fn build(&mut self, (x, z): (i32, i32), kind: MarkerKind) {
        let build = self.scale.build;
        let reach = 4 * build;
        let ground = self.top(x, z).unwrap_or(SEA_Y).max(SEA_Y + 1);
        for dz in -reach..=reach {
            for dx in -reach..=reach {
                if let Some(column) = self.column_mut(x + dx, z + dz)
                    && dx * dx + dz * dz <= reach * reach
                {
                    column.ground = ground;
                    column.surface = GRASS;
                }
            }
        }
        for (bx, by, bz, block) in landmark(kind) {
            for part in 0..build * build * build {
                let (px, py, pz) = (part % build, part / build % build, part / build / build);
                let at = (x + bx * build + px, z + bz * build + pz);
                let y = ground + 1 + by * build + py;
                self.buildings.entry(at).or_default().push((y, block));
            }
        }
    }

    /// Lays a winding path between two cells, on planks over the water.
    pub fn pave(&mut self, from: (i32, i32), to: (i32, i32)) {
        let steps = (to.0 - from.0).abs().max((to.1 - from.1).abs()).max(1);
        let width = self.scale.build;
        for step in 0..=steps {
            let along = |a: i32, b: i32| {
                f64::from(a) + f64::from(b - a) * f64::from(step) / f64::from(steps)
            };
            let (line_x, line_z) = (along(from.0, to.0), along(from.1, to.1));
            // The middle of a path strays the most, and both ends stay put.
            let progress = f64::from(step) / f64::from(steps);
            let swing = (progress * std::f64::consts::PI).sin() * f64::from(steps) * 0.18;
            // In world blocks, so that a path is at the same place on every model.
            let per_cell = f64::from(self.scale.blocks_per_cell);
            let wander = swing
                * fractal(
                    PATH_SEED,
                    line_x * per_cell,
                    line_z * per_cell,
                    f64::from(steps) * per_cell,
                );
            let (x, z) = ((line_x + wander) as i32, (line_z - wander) as i32);
            for dz in 0..width {
                for dx in 0..width {
                    if let Some(column) = self.column_mut(x + dx, z + dz) {
                        if column.is_land() {
                            column.surface = PATH;
                        } else {
                            column.ground = SEA_Y;
                            column.surface = BRIDGE;
                        }
                    }
                }
            }
        }
    }

    /// The blocks of a column in a look, from the bottom up. A later one takes over from
    /// an earlier one at the same height.
    fn paint(&self, x: i32, z: i32, look: ChunkLook, set: &mut impl FnMut(i32, i32)) {
        // Clouds drift over open land and past the shore, so they answer to no column.
        if look.zone == ZoneLook::Clouds {
            for y in self.clouds.get(&(x, z)).into_iter().flatten() {
                set(*y, CLOUD);
            }
        }
        let Some(column) = self.column(x, z) else {
            return;
        };
        let closed = column.state != RegionState::Discovered;
        let tint = |block| {
            if closed && look.zone == ZoneLook::Faded {
                faded(block)
            } else if look.blocks == Blocks::Real {
                real_block(block)
            } else {
                column.state.tint(block)
            }
        };
        for y in column.bottom..column.ground {
            set(y, tint(cliff_block(column.surface, column.ground - y)));
        }
        set(column.ground, tint(column.surface));
        for y in column.ground + 1..=SEA_Y {
            set(y, tint(column.water));
        }
        for (y, block) in self.buildings.get(&(x, z)).into_iter().flatten() {
            set(*y, tint(*block));
        }
        if closed {
            self.zone_look(look.zone, x, z, column, set);
        }
    }

    /// What it takes to turn a chunk from one look to another: the blocks that differ,
    /// section by section. A look of the zones leaves all the open land alone.
    pub fn repaint(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        from: ChunkLook,
        to: ChunkLook,
    ) -> Vec<SectionChange> {
        let mut sections: BTreeMap<i32, Vec<VarI64>> = BTreeMap::new();
        for local_z in 0..16u8 {
            for local_x in 0..16u8 {
                let (x, z) = (
                    chunk_x * 16 + i32::from(local_x),
                    chunk_z * 16 + i32::from(local_z),
                );
                let closed = self
                    .column(x, z)
                    .is_some_and(|column| column.state != RegionState::Discovered);
                if from.blocks == to.blocks && !closed && !self.clouds.contains_key(&(x, z)) {
                    continue;
                }
                let column_in = |look| {
                    let mut blocks = BTreeMap::new();
                    self.paint(x, z, look, &mut |y, block| {
                        blocks.insert(y, block);
                    });
                    blocks
                };
                let (before, after) = (column_in(from), column_in(to));
                let heights = before.keys().chain(after.keys()).copied();
                for y in heights.collect::<BTreeSet<_>>() {
                    let block = after.get(&y).copied().unwrap_or(b::AIR);
                    if before.get(&y).copied().unwrap_or(b::AIR) != block {
                        let height = y.rem_euclid(16) as u8;
                        sections
                            .entry(y.div_euclid(16))
                            .or_default()
                            .push(SectionBlocksUpdate::block(local_x, height, local_z, block));
                    }
                }
            }
        }
        sections
            .into_iter()
            .map(|(section, blocks)| SectionChange { section, blocks })
            .collect()
    }

    /// One chunk of the model, centred on chunk `(0, 0)`. Off the model it is empty,
    /// which the client needs too: it only draws chunks with all their neighbours.
    pub fn chunk(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        biome: BiomeId,
        count: usize,
        look: ChunkLook,
    ) -> Chunk {
        let mut builder = ChunkBuilder::new(chunk_x, chunk_z)
            .biome(biome.0)
            .biome_registry_size(count);
        for local_z in 0..16u8 {
            for local_x in 0..16u8 {
                let (x, z) = (
                    chunk_x * 16 + i32::from(local_x),
                    chunk_z * 16 + i32::from(local_z),
                );
                self.paint(x, z, look, &mut |y, block| {
                    builder.set_block(local_x, y, local_z, block);
                });
            }
        }
        let mut chunk = builder.build();
        // The biome lights everything alike.
        chunk.light = LightData::empty();
        chunk
    }
}

#[cfg(test)]
mod tests {
    use voidmc::{DefaultWorldGenerator, WorldGenerator};

    use super::*;
    use crate::regions::Region;
    use crate::testing::Flat;

    const SEA: i32 = 62;

    fn open() -> Regions {
        Regions::new(vec![Region {
            name: "Tout",
            site: [0.0, 0.0],
            state: RegionState::Discovered,
        }])
    }

    fn scene_of(generator: &dyn WorldGenerator, scale: Scale) -> Scene {
        Scene::new(
            &Terrain::sample(generator, &scale.window()),
            SEA,
            scale,
            &open(),
        )
    }

    fn scene(generator: &dyn WorldGenerator) -> Scene {
        scene_of(generator, OVERVIEW)
    }

    fn land() -> Flat {
        Flat::new(SEA + 12, BiomeId::plains())
    }

    fn sea() -> Flat {
        Flat::new(SEA - 30, BiomeId::plains())
    }

    fn demo_scene() -> Scene {
        let terrain = Terrain::sample(&DefaultWorldGenerator::default(), &OVERVIEW.window());
        Scene::new(&terrain, SEA, OVERVIEW, &Regions::demo())
    }

    fn column_in(scene: &Scene, x: i32, z: i32, look: ChunkLook) -> BTreeMap<i32, i32> {
        let mut blocks = BTreeMap::new();
        scene.paint(x, z, look, &mut |y, block| {
            blocks.insert(y, block);
        });
        blocks.retain(|_, block| *block != b::AIR);
        blocks
    }

    const ZONES: [ZoneLook; 4] = ZoneLook::ALL;

    /// A repaint stands in for sending the chunk again, so it has to end on the same blocks.
    #[test]
    fn a_repaint_ends_on_the_blocks_of_the_new_look() {
        let scene = demo_scene();
        let reach = scene.chunk_radius();
        let looks = ZONES.map(|zone| ChunkLook {
            zone,
            blocks: Blocks::Flat,
        });
        let real = ChunkLook {
            zone: ZoneLook::Clouds,
            blocks: Blocks::Real,
        };
        for (from, to) in [(looks[0], looks[1]), (looks[2], looks[3]), (looks[3], real)] {
            for (chunk_x, chunk_z) in [(-reach / 2, 0), (0, reach / 2), (1, 1), (reach / 2, -2)] {
                let mut sections = BTreeMap::new();
                for change in scene.repaint(chunk_x, chunk_z, from, to) {
                    sections.insert(change.section, change.blocks);
                }
                for local in 0..256 {
                    let (x, z) = (chunk_x * 16 + local % 16, chunk_z * 16 + local / 16);
                    let mut column = column_in(&scene, x, z, from);
                    for (section, blocks) in &sections {
                        for VarI64(entry) in blocks {
                            let (at_x, at_z) = ((entry >> 8) & 15, (entry >> 4) & 15);
                            if (at_x, at_z) == (i64::from(local % 16), i64::from(local / 16)) {
                                let y = section * 16 + (entry & 15) as i32;
                                column.insert(y, (entry >> 12) as i32);
                            }
                        }
                    }
                    column.retain(|_, block| *block != b::AIR);
                    assert_eq!(column, column_in(&scene, x, z, to), "{x} {z}");
                }
            }
        }
    }

    #[test]
    fn a_look_of_the_zones_leaves_the_open_land_alone() {
        let terrain = Terrain::sample(&DefaultWorldGenerator::default(), &OVERVIEW.window());
        let scene = Scene::new(&terrain, SEA, OVERVIEW, &open());
        let [from, to] = [ZoneLook::Clouds, ZoneLook::Faded].map(|zone| ChunkLook {
            zone,
            blocks: Blocks::Flat,
        });
        assert!(scene.repaint(0, 0, from, to).is_empty());
    }

    #[test]
    fn both_scales_cover_the_same_world() {
        let span = |scale: Scale| f64::from(scale.radius * i32::from(scale.blocks_per_cell));
        for scale in [OVERVIEW, DETAIL] {
            assert!((span(scale) - WORLD_RADIUS).abs() <= f64::from(scale.blocks_per_cell));
        }
    }

    #[test]
    fn the_island_ends_inside_its_radius() {
        let scene = scene(&land());
        let radius = OVERVIEW.radius;
        assert!(scene.top(0, 0).is_some());
        assert!(scene.top(radius - 1, radius - 1).is_none());
        assert!(scene.top(radius, 0).is_none());
    }

    #[test]
    fn the_rim_is_no_circle() {
        let scene = scene(&land());
        let reach = |dx: i32, dz: i32| {
            (1..OVERVIEW.radius)
                .take_while(|r| scene.top(r * dx, r * dz).is_some())
                .count()
        };
        let reaches = [reach(1, 0), reach(-1, 0), reach(0, 1), reach(0, -1)];
        assert!(reaches.iter().any(|r| *r != reaches[0]), "{reaches:?}");
    }

    #[test]
    fn land_rises_by_terraces() {
        let scene = scene(&land());
        let tops: Vec<_> = (-30..30).filter_map(|x| scene.top(x, 0)).collect();
        assert!(tops.iter().all(
            |top| (*top - SEA_Y - 1) % OVERVIEW.rise == 0 || *top == SEA_Y + 1 + OVERVIEW.build
        ));
    }

    #[test]
    fn the_sea_is_flat() {
        let scene = scene(&sea());
        assert_eq!(scene.top(0, 0), Some(SEA_Y));
    }

    #[test]
    fn the_sea_never_spills_over_the_rim() {
        let scene = scene(&sea());
        let rim = (0..OVERVIEW.radius)
            .rev()
            .find(|x| scene.top(*x, 0).is_some());
        assert!(scene.column(rim.unwrap(), 0).unwrap().is_land());
    }

    #[test]
    fn extreme_terrain_stays_inside_the_dimension() {
        let high = scene(&Flat::new(10_000, BiomeId::plains()));
        assert_eq!(high.top(0, 0), Some(OVERVIEW.max_y));
        let deep = scene(&Flat::new(-10_000, BiomeId::plains()));
        assert!(deep.column(0, 0).unwrap().bottom >= LOWEST_Y);
    }

    #[test]
    fn the_underside_is_thinner_at_the_rim() {
        let scene = scene(&land());
        let rim = (0..OVERVIEW.radius)
            .rev()
            .find(|x| scene.top(*x, 0).is_some());
        let bottom = |x| scene.column(x, 0).unwrap().bottom;
        assert!(bottom(rim.unwrap()) > bottom(0));
    }

    #[test]
    fn the_surface_depends_on_elevation_and_biome() {
        let plains = BiomeId::plains();
        let desert = BiomeId::named("desert").unwrap();
        assert_eq!(surface_block(plains, 4, 1), GRASS);
        assert_eq!(surface_block(desert, 4, 1), SAND);
        assert_eq!(surface_block(plains, 1, 0), SAND);
        assert_eq!(surface_block(plains, 20, ROCK_TERRACE), ROCK);
        assert_eq!(surface_block(plains, 30, SNOW_TERRACE), SNOW);
    }

    #[test]
    fn cliffs_show_soil_then_fade_to_black() {
        assert_eq!(cliff_block(GRASS, 1), SOIL);
        assert_eq!(cliff_block(ROCK, 1), b::LIGHT_GRAY_CONCRETE);
        assert_eq!(cliff_block(GRASS, DARK_DEPTH), b::GRAY_CONCRETE);
        assert_eq!(cliff_block(GRASS, BLACK_DEPTH), b::BLACK_CONCRETE);
    }

    #[test]
    fn smoothing_keeps_a_flat_field_and_softens_a_spike() {
        let mut flat = vec![3.0; 25];
        smooth(&mut flat, 5, 1);
        assert!(flat.iter().all(|v| (*v - 3.0).abs() < 1e-9));
        let mut spike = vec![0.0; 25];
        spike[12] = 9.0;
        smooth(&mut spike, 5, 1);
        assert_eq!(spike[12], 1.0);
    }

    #[test]
    fn a_path_is_unbroken_and_bridges_the_sea() {
        let mut scene = scene(&sea());
        scene.pave((-20, 0), (20, 0));
        let start = scene.column(-20, 0).unwrap();
        assert_eq!(start.surface, BRIDGE);
        assert_eq!(start.ground, SEA_Y);
        let end = scene.column(20, 0).unwrap();
        assert_eq!(end.surface, BRIDGE);
    }

    #[test]
    fn a_landmark_stands_on_level_land() {
        let mut scene = scene(&sea());
        scene.build((0, 0), MarkerKind::Lighthouse);
        assert_eq!(scene.top(0, 0), Some(SEA_Y + 1));
        assert_eq!(scene.top(3, 0), Some(SEA_Y + 1));
        assert!(scene.buildings.contains_key(&(0, 0)));
    }

    #[test]
    fn a_landmark_grows_with_the_scale() {
        let mut detail = scene_of(&land(), DETAIL);
        detail.build((0, 0), MarkerKind::Lighthouse);
        assert_eq!(detail.buildings[&(1, 1)].len(), 7 * 2);
    }

    #[test]
    fn a_region_tints_the_blocks_it_covers() {
        let locked = Regions::new(vec![Region {
            name: "Fermee",
            site: [0.0, 0.0],
            state: RegionState::Locked,
        }]);
        let generator = land();
        let terrain = Terrain::sample(&generator, &OVERVIEW.window());
        let scene = Scene::new(&terrain, SEA, OVERVIEW, &locked);
        assert_eq!(scene.column(0, 0).unwrap().state, RegionState::Locked);
    }

    fn closed_scene(scale: Scale) -> Scene {
        let generator = land();
        let terrain = Terrain::sample(&generator, &scale.window());
        Scene::new(&terrain, SEA, scale, &Regions::demo())
    }

    fn cloud_tops(scene: &Scene) -> impl Iterator<Item = ((i32, i32), i32)> + '_ {
        scene
            .clouds
            .iter()
            .map(|(cell, heights)| (*cell, heights.iter().copied().max().unwrap_or(0)))
    }

    #[test]
    fn clouds_are_the_same_every_time_and_on_every_model() {
        let (first, second) = (closed_scene(OVERVIEW), closed_scene(OVERVIEW));
        assert_eq!(first.clouds, second.clouds);
        assert!(!first.clouds.is_empty());
        assert_eq!(cloud_list(&Regions::demo()), cloud_list(&Regions::demo()));
    }

    /// Their outlines, each moved to the same place, as a game would draw them.
    #[test]
    fn no_two_clouds_are_the_same_drawing() {
        let clouds = cloud_list(&Regions::demo());
        assert!(clouds.len() >= 8, "{}", clouds.len());
        let drawing = |cloud: &Cloud| {
            let [west, north, east, south] = cloud.bounds();
            let cell = |from: f64, step: i32| from + f64::from(step) * CLOUD_PIXEL;
            let steps = |from: f64, to: f64| ((to - from) / CLOUD_PIXEL).ceil() as i32;
            (0..steps(north, south))
                .flat_map(|z| (0..steps(west, east)).map(move |x| (x, z)))
                .map(|(x, z)| cloud.layers_at(cell(west, x), cell(north, z)))
                .collect::<Vec<_>>()
        };
        let drawings: BTreeSet<_> = clouds.iter().map(drawing).collect();
        assert_eq!(drawings.len(), clouds.len());
    }

    #[test]
    fn clouds_come_in_all_sizes_and_up_to_four_layers() {
        let clouds = cloud_list(&Regions::demo());
        let width = |cloud: &Cloud| cloud.bounds()[2] - cloud.bounds()[0];
        let widths: Vec<_> = clouds.iter().map(width).collect();
        let (least, most) = widths
            .iter()
            .fold((f64::MAX, 0.0_f64), |(low, high), width| {
                (low.min(*width), high.max(*width))
            });
        assert!(most > least * 2.0, "{least} {most}");
        let layers: BTreeSet<_> = clouds.iter().map(|cloud| cloud.layers).collect();
        assert!(layers.len() >= 3 && layers.iter().all(|n| (1..=4).contains(n)));
    }

    #[test]
    fn a_cloud_is_of_one_colour_in_every_region() {
        let scene = closed_scene(DETAIL);
        let look = ChunkLook {
            zone: ZoneLook::Clouds,
            ..ChunkLook::default()
        };
        for ((x, z), heights) in &scene.clouds {
            let column = column_in(&scene, *x, *z, look);
            assert!(heights.iter().all(|y| column[y] == CLOUD), "{x} {z}");
        }
    }

    #[test]
    fn a_cloud_is_a_few_flat_layers() {
        for scale in [OVERVIEW, MIDDLE, DETAIL] {
            let scene = closed_scene(scale);
            for blocks in scene.clouds.values() {
                let layers = blocks.len() as i32 / scale.build;
                assert!((1..=4).contains(&layers), "{layers}");
            }
        }
    }

    #[test]
    fn only_closed_regions_have_clouds() {
        assert!(scene(&land()).clouds.is_empty());
        assert!(!closed_scene(OVERVIEW).clouds.is_empty());
    }

    #[test]
    fn clouds_stay_apart_and_on_the_model() {
        let clouds = cloud_list(&Regions::demo());
        for (index, cloud) in clouds.iter().enumerate() {
            let [west, north, east, south] = cloud.bounds();
            assert!(west >= -WORLD_RADIUS && north >= -WORLD_RADIUS);
            assert!(east <= WORLD_RADIUS && south <= WORLD_RADIUS);
            assert!(clouds[..index].iter().all(|other| !cloud.overlaps(other)));
        }
    }

    #[test]
    fn a_cloud_stays_whole_past_the_border_of_its_region() {
        let scene = closed_scene(OVERVIEW);
        let chunk = |x, z, zone| {
            let look = ChunkLook {
                zone,
                blocks: Blocks::Flat,
            };
            scene.chunk(x, z, BiomeId::plains(), 1, look)
        };
        let (x, z) = scene
            .clouds
            .keys()
            .copied()
            .find(|(x, z)| {
                scene
                    .column(*x, *z)
                    .is_none_or(|column| column.state == RegionState::Discovered)
            })
            .expect("a cloud past a border");
        let y = scene.clouds[&(x, z)][0];
        let built = chunk(x.div_euclid(16), z.div_euclid(16), ZoneLook::Clouds);
        let (local_x, local_z) = (x.rem_euclid(16) as u8, z.rem_euclid(16) as u8);
        let (section, local_y) = ((y + 64) as usize / 16, (y + 64).rem_euclid(16) as u8);
        let found = built.sections[section].get_block_state(local_x, local_y, local_z);
        assert_eq!(found, CLOUD);
    }

    #[test]
    fn clouds_clear_the_land_and_a_zoomed_camera() {
        for scale in [OVERVIEW, MIDDLE, DETAIL] {
            let scene = closed_scene(scale);
            for ((x, z), top) in cloud_tops(&scene) {
                if let Some(column) = scene.column(x, z) {
                    assert!(top > column.ground, "{x} {z}");
                }
            }
        }
    }

    #[test]
    fn a_walkable_cell_has_room_around_it() {
        let scene = scene(&land());
        assert!(scene.walkable(0, 0));
        let rim = (0..OVERVIEW.radius)
            .rev()
            .find(|x| scene.top(*x, 0).is_some())
            .unwrap();
        assert!(!scene.walkable(rim, 0));
        let sea = scene_of(&sea(), OVERVIEW);
        assert!(!sea.walkable(0, 0));
    }

    #[test]
    fn a_landmark_and_the_land_around_it_are_not_walked_on() {
        let mut scene = scene(&land());
        assert!(scene.walkable(0, 0));
        scene.build((0, 0), MarkerKind::Lighthouse);
        assert!(!scene.walkable(0, 0));
        assert!(!scene.walkable(1, 0));
        assert!(scene.walkable(2, 0));
    }

    #[test]
    fn a_figure_is_shown_next_to_what_it_cannot_stand_on() {
        let mut scene = scene(&land());
        scene.build((0, 0), MarkerKind::Lighthouse);
        let (x, z) = scene.stand_near(0, 0).unwrap();
        assert_eq!(x.abs().max(z.abs()), 1);
        assert_eq!(scene_of(&sea(), OVERVIEW).stand_near(0, 0), None);
    }

    #[test]
    fn a_path_is_at_the_same_place_on_every_model() {
        let bridge = |scale: Scale| {
            let mut scene = scene_of(&sea(), scale);
            let cell = |world: i32| world / i32::from(scale.blocks_per_cell);
            scene.pave((cell(-240), cell(-120)), (cell(240), cell(160)));
            let radius = scale.radius;
            (-radius..radius)
                .flat_map(|x| (-radius..radius).map(move |z| (x, z)))
                .filter(|(x, z)| scene.column(*x, *z).is_some_and(|c| c.surface == BRIDGE))
                .map(|(x, z)| [scale.world(x), scale.world(z)])
                .collect::<Vec<_>>()
        };
        let (coarse, fine) = (bridge(OVERVIEW), bridge(DETAIL));
        assert!(!coarse.is_empty());
        for [x, z] in coarse {
            let closest = fine
                .iter()
                .map(|[fine_x, fine_z]| (fine_x - x).hypot(fine_z - z))
                .fold(f64::MAX, f64::min);
            assert!(closest <= 16.0, "{x} {z}: {closest}");
        }
    }

    #[test]
    fn the_real_world_is_mostly_on_the_model() {
        let scene = scene(&DefaultWorldGenerator::default());
        let cells = (-OVERVIEW.radius..OVERVIEW.radius)
            .flat_map(|x| (-OVERVIEW.radius..OVERVIEW.radius).map(move |z| (x, z)));
        let on_model = cells.filter(|(x, z)| scene.top(*x, *z).is_some()).count();
        assert!(on_model > 6_000, "{on_model}");
    }
}
