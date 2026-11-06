//! The block model of the world: a floating island, drawn rather than measured.
//!
//! The geography is the generator's. Its land and sea stay where they are, near
//! enough, but its waves are bent and smoothed, because a regular world reads as a
//! chequerboard at this size.

use std::collections::HashMap;
use std::num::{NonZeroU8, NonZeroU16};

use voidmc::BiomeId;
use voidmc_data::v26_1_2::blocks as b;
use voidmc_protocol::clientbound::{Chunk, ChunkBuilder, LightData};

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
const CLOUD_SEED: u32 = 70;
/// Each cell of a grid this wide may hold one cloud, in world blocks: clouds are the same
/// on every model.
const CLOUD_SPACING: f64 = 84.0;
/// The share of the cells that do.
const CLOUD_CHANCE: f64 = 0.8;
const CLOUD_SMALLEST: f64 = 18.0;
const CLOUD_LARGEST: f64 = 30.0;
/// The two side puffs of a cloud, as shares of its middle one, which make it long.
const CLOUD_SIDE_REACH: f64 = 1.0;
const CLOUD_SIDE_SIZE: f64 = 0.68;
/// How deep into a cloud each of its layers starts, from the bottom up.
const CLOUD_LAYERS: [f64; 3] = [0.14, 0.0, 0.38];
/// Over the sea, as a share of the model's height over the sea.
const CLOUD_HEIGHT: f64 = 0.62;
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
    /// The blocks of the clouds over each column, worked out once.
    clouds: HashMap<(i32, i32), Vec<(i32, i32)>>,
}

/// What a cloud is made of: its light side and its one shade.
fn cloud_blocks(state: RegionState) -> (i32, i32) {
    match state {
        RegionState::Locked => (b::LIGHT_GRAY_CONCRETE, b::GRAY_CONCRETE),
        RegionState::Undiscovered | RegionState::Discovered => {
            (b::WHITE_CONCRETE, b::LIGHT_GRAY_CONCRETE)
        }
    }
}

/// A small cloud, as in a game: three puffs in a row.
#[derive(Debug, Clone, Copy)]
struct Cloud {
    at: [f64; 2],
    size: f64,
}

impl Cloud {
    /// How deep a point is in the cloud, from 0 on its edge to 1 in its very middle.
    fn depth(self, x: f64, z: f64) -> f64 {
        let [at_x, at_z] = self.at;
        let reach = self.size * CLOUD_SIDE_REACH;
        let puffs = [
            (at_x, self.size),
            (at_x - reach, self.size * CLOUD_SIDE_SIZE),
            (at_x + reach, self.size * CLOUD_SIDE_SIZE),
        ];
        puffs
            .into_iter()
            .map(|(puff_x, size)| 1.0 - (x - puff_x).hypot(z - at_z) / size)
            .fold(f64::MIN, f64::max)
    }
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
        let clouds = Self::clouds(&columns, scale);
        Self {
            scale,
            columns,
            buildings: HashMap::new(),
            clouds,
        }
    }

    /// Small clouds scattered over the closed zones, each a few flat layers. A cloud whose
    /// middle is over a zone hangs there whole, past its border if need be.
    fn clouds(columns: &[Option<Column>], scale: Scale) -> HashMap<(i32, i32), Vec<(i32, i32)>> {
        let size = usize::from(scale.size());
        let state_at = |x: f64, z: f64| {
            let cell = |world: f64| (scale.cells(world).floor() as i32 + scale.radius) as usize;
            let (x, z) = (cell(x), cell(z));
            (x < size && z < size)
                .then(|| columns[z * size + x].map(|column| column.state))
                .flatten()
                .filter(|state| *state != RegionState::Discovered)
        };
        let reach = f64::from(scale.radius) * f64::from(scale.blocks_per_cell);
        let cells = (reach / CLOUD_SPACING).ceil() as i32;
        let height = SEA_Y + (f64::from(scale.max_y - SEA_Y) * CLOUD_HEIGHT) as i32;
        let mut clouds: HashMap<(i32, i32), Vec<(i32, i32)>> = HashMap::new();
        for cell_z in -cells..=cells {
            for cell_x in -cells..=cells {
                let [jitter_x, jitter_z, chance, grow] = scatter(CLOUD_SEED, cell_x, cell_z);
                let at = [
                    (f64::from(cell_x) + 0.25 + jitter_x * 0.5) * CLOUD_SPACING,
                    (f64::from(cell_z) + 0.25 + jitter_z * 0.5) * CLOUD_SPACING,
                ];
                let Some(state) = state_at(at[0], at[1]).filter(|_| chance < CLOUD_CHANCE) else {
                    continue;
                };
                let radius = CLOUD_SMALLEST + (CLOUD_LARGEST - CLOUD_SMALLEST) * grow;
                let cloud = Cloud { at, size: radius };
                let (light, shade) = cloud_blocks(state);
                let span = scale.cells(radius * (1.0 + CLOUD_SIDE_REACH)).ceil() as i32 + 1;
                let (middle_x, middle_z) = (
                    scale.cells(at[0]).floor() as i32,
                    scale.cells(at[1]).floor() as i32,
                );
                for z in middle_z - span..=middle_z + span {
                    for x in middle_x - span..=middle_x + span {
                        let depth = cloud.depth(scale.world(x), scale.world(z));
                        for (layer, start) in CLOUD_LAYERS.iter().enumerate() {
                            if depth <= *start {
                                continue;
                            }
                            let block = if layer == 0 { shade } else { light };
                            let bottom = height + layer as i32 * scale.build;
                            let column = clouds.entry((x, z)).or_default();
                            column.extend((0..scale.build).map(|y| (bottom + y, block)));
                        }
                    }
                }
            }
        }
        clouds
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
            ZoneLook::Clouds => {
                for (y, block) in self.clouds.get(&(x, z)).into_iter().flatten() {
                    set(*y, *block);
                }
            }
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
            ZoneLook::Faded => {}
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
            let wander = fractal(PATH_SEED, line_x, line_z, f64::from(steps)) * swing;
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
                let Some(column) = self.column(x, z) else {
                    continue;
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
                    let block = cliff_block(column.surface, column.ground - y);
                    builder.set_block(local_x, y, local_z, tint(block));
                }
                builder.set_block(local_x, column.ground, local_z, tint(column.surface));
                for y in column.ground + 1..=SEA_Y {
                    builder.set_block(local_x, y, local_z, tint(column.water));
                }
                for (y, block) in self.buildings.get(&(x, z)).into_iter().flatten() {
                    builder.set_block(local_x, *y, local_z, tint(*block));
                }
                if closed {
                    let mut set = |y, block| builder.set_block(local_x, y, local_z, block);
                    self.zone_look(look.zone, x, z, column, &mut set);
                }
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

    #[test]
    fn the_real_world_is_mostly_on_the_model() {
        let scene = scene(&DefaultWorldGenerator::default());
        let cells = (-OVERVIEW.radius..OVERVIEW.radius)
            .flat_map(|x| (-OVERVIEW.radius..OVERVIEW.radius).map(move |z| (x, z)));
        let on_model = cells.filter(|(x, z)| scene.top(*x, *z).is_some()).count();
        assert!(on_model > 6_000, "{on_model}");
    }
}
