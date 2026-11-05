//! Turns sampled terrain into RGBA pixels, one pixel per cell.

use std::collections::HashMap;
use std::num::NonZeroU8;

use voidmc::BiomeId;

use crate::terrain::Terrain;

type Rgb = [u8; 3];

const OPAQUE: u8 = 255;

const DEFAULT_LAND: Rgb = [121, 170, 84];
const SHALLOW_WATER: Rgb = [78, 150, 214];
const DEEP_WATER: Rgb = [26, 62, 136];
const ROCK: Rgb = [128, 124, 120];
const SNOW: Rgb = [240, 244, 250];

/// Depth at which water reaches [`DEEP_WATER`].
const FULL_DEPTH: f32 = 24.0;
/// Blocks above sea level where land starts turning to rock, and is bare.
const ROCK_START: f32 = 48.0;
const ROCK_FULL: f32 = 64.0;
/// Blocks above sea level where rock starts turning to snow, and is white.
const SNOW_START: f32 = 72.0;
const SNOW_FULL: f32 = 84.0;
/// Land brightens from `LOWLAND_LIGHT` at sea level by `ELEVATION_LIGHT`.
const LOWLAND_LIGHT: f32 = 0.86;
const ELEVATION_LIGHT: f32 = 0.24;
const ELEVATION_RANGE: f32 = 40.0;
/// Light gained per block of rise per block of distance, facing north-west.
const SHADE_GAIN: f32 = 0.3;
const SHADE_LIMIT: f32 = 0.35;

const DEFAULT_QUANTUM: u8 = 8;

/// Vanilla biomes with a colour of their own. The rest draw as
/// [`DEFAULT_LAND`], which includes custom biomes.
const BIOME_COLORS: &[(&str, Rgb)] = &[
    ("plains", DEFAULT_LAND),
    ("sunflower_plains", [134, 178, 84]),
    ("meadow", [120, 184, 104]),
    ("forest", [72, 130, 62]),
    ("flower_forest", [88, 146, 72]),
    ("birch_forest", [98, 150, 78]),
    ("dark_forest", [46, 92, 46]),
    ("taiga", [76, 120, 96]),
    ("snowy_taiga", [190, 210, 214]),
    ("snowy_plains", [226, 234, 240]),
    ("ice_spikes", [196, 226, 244]),
    ("jungle", [58, 144, 44]),
    ("swamp", [84, 108, 66]),
    ("savanna", [176, 164, 84]),
    ("desert", [222, 206, 148]),
    ("beach", [232, 218, 164]),
    ("badlands", [190, 104, 58]),
    ("stony_shore", ROCK),
    ("windswept_hills", [112, 134, 112]),
];

/// How terrain turns into colours.
pub struct Style {
    sea_level: i32,
    quantum: NonZeroU8,
    biomes: HashMap<BiomeId, Rgb>,
}

impl Style {
    /// `WorldGenerator` does not expose its water level, so the caller gives
    /// it. Cells at or below it draw as water. Colours are rounded to steps of
    /// 8 per channel, which merges near-identical neighbours.
    pub fn new(sea_level: i32) -> Self {
        Self {
            sea_level,
            quantum: NonZeroU8::new(DEFAULT_QUANTUM).unwrap_or(NonZeroU8::MIN),
            biomes: BIOME_COLORS
                .iter()
                .filter_map(|(name, color)| Some((BiomeId::named(name)?, *color)))
                .collect(),
        }
    }

    /// Rounds each channel to a multiple of `quantum`. `1` keeps every colour.
    pub fn quantum(mut self, quantum: NonZeroU8) -> Self {
        self.quantum = quantum;
        self
    }

    fn water(&self, height: i32) -> [f32; 3] {
        let depth = (self.sea_level - height) as f32 / FULL_DEPTH;
        mix(
            SHALLOW_WATER.map(f32::from),
            DEEP_WATER.map(f32::from),
            depth,
        )
    }

    fn land(&self, biome: BiomeId, height: i32) -> [f32; 3] {
        let color = self.biomes.get(&biome).copied().unwrap_or(DEFAULT_LAND);
        let elevation = (height - self.sea_level) as f32;
        let color = mix(
            color.map(f32::from),
            ROCK.map(f32::from),
            ramp(elevation, ROCK_START, ROCK_FULL),
        );
        let color = mix(
            color,
            SNOW.map(f32::from),
            ramp(elevation, SNOW_START, SNOW_FULL),
        );
        let light = LOWLAND_LIGHT + ELEVATION_LIGHT * ramp(elevation, 0.0, ELEVATION_RANGE);
        color.map(|channel| channel * light)
    }

    fn round(&self, channel: f32) -> u8 {
        let quantum = f32::from(self.quantum.get());
        ((channel / quantum).round() * quantum).clamp(0.0, 255.0) as u8
    }
}

fn ramp(value: f32, start: f32, end: f32) -> f32 {
    ((value - start) / (end - start)).clamp(0.0, 1.0)
}

fn mix(from: [f32; 3], to: [f32; 3], amount: f32) -> [f32; 3] {
    let amount = amount.clamp(0.0, 1.0);
    std::array::from_fn(|i| from[i] + (to[i] - from[i]) * amount)
}

/// Opaque RGBA pixels, row by row from the north-west corner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    width: u16,
    height: u16,
    rgba: Vec<u8>,
}

impl Image {
    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    /// `width * height * 4` bytes.
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    pub fn pixel(&self, col: u16, row: u16) -> [u8; 4] {
        let start = (usize::from(row) * usize::from(self.width) + usize::from(col)) * 4;
        [
            self.rgba[start],
            self.rgba[start + 1],
            self.rgba[start + 2],
            self.rgba[start + 3],
        ]
    }
}

/// Light on a cell lit from the north-west: 1.0 on flat ground, more on a
/// slope that faces the light.
fn shade(terrain: &Terrain, col: u16, row: u16) -> f32 {
    let height = terrain.height(col, row);
    let rise = (height - terrain.height_west(col, row)) + (height - terrain.height_north(col, row));
    let slope = rise as f32 / f32::from(terrain.window().scale());
    1.0 + (slope * SHADE_GAIN).clamp(-SHADE_LIMIT, SHADE_LIMIT)
}

pub fn render(terrain: &Terrain, style: &Style) -> Image {
    let (width, height) = (terrain.window().cols(), terrain.window().rows());
    let mut rgba = Vec::with_capacity(usize::from(width) * usize::from(height) * 4);
    for row in 0..height {
        for col in 0..width {
            let surface = terrain.height(col, row);
            let color = if surface <= style.sea_level {
                style.water(surface)
            } else {
                let light = shade(terrain, col, row);
                style
                    .land(terrain.biome(col, row), surface)
                    .map(|channel| channel * light)
            };
            rgba.extend(color.map(|channel| style.round(channel)));
            rgba.push(OPAQUE);
        }
    }
    Image {
        width,
        height,
        rgba,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use voidmc::{DefaultWorldGenerator, WorldGenerator};

    use super::*;
    use crate::testing::{Flat, Shaped, window};
    use crate::window::Direction;

    const SEA: i32 = 62;

    fn draw(generator: &dyn WorldGenerator, cols: u16, rows: u16) -> Image {
        let terrain = Terrain::sample(generator, &window(0, 0, cols, rows, 4));
        render(&terrain, &Style::new(SEA))
    }

    fn flat_pixel(height: i32, biome: &str) -> [u8; 4] {
        let biome = BiomeId::named(biome).unwrap();
        draw(&Flat::new(height, biome), 1, 1).pixel(0, 0)
    }

    fn brightness(pixel: [u8; 4]) -> u32 {
        pixel[..3].iter().copied().map(u32::from).sum()
    }

    #[test]
    fn every_cell_gets_one_opaque_pixel() {
        let image = draw(&DefaultWorldGenerator::default(), 12, 7);
        assert_eq!((image.width(), image.height()), (12, 7));
        assert_eq!(image.rgba().len(), 12 * 7 * 4);
        assert!(image.rgba().chunks(4).all(|pixel| pixel[3] == OPAQUE));
    }

    #[test]
    fn pixels_run_west_to_east_then_north_to_south() {
        let generator = Shaped {
            height: |x: i32, _| if x >= 0 { 70 } else { SEA - 5 },
            biome: |_, _| BiomeId::plains(),
        };
        let image = draw(&generator, 4, 2);
        let is_water = |pixel: [u8; 4]| pixel[2] > pixel[1];
        assert!(is_water(image.pixel(0, 1)));
        assert!(!is_water(image.pixel(3, 1)));
        assert_eq!(&image.rgba()[12..16], image.pixel(3, 0));
    }

    #[test]
    fn water_stops_at_sea_level_and_darkens_with_depth() {
        let at_sea_level = flat_pixel(SEA, "plains");
        assert!(at_sea_level[2] > at_sea_level[1]);
        let above = flat_pixel(SEA + 1, "plains");
        assert!(above[1] > above[2]);
        assert!(brightness(flat_pixel(SEA - 20, "plains")) < brightness(at_sea_level));
    }

    #[test]
    fn biomes_have_their_own_colour() {
        let colors: HashSet<_> = ["plains", "desert", "dark_forest", "snowy_plains"]
            .map(|biome| flat_pixel(70, biome))
            .into_iter()
            .collect();
        assert_eq!(colors.len(), 4);
    }

    #[test]
    fn every_listed_biome_exists_in_the_vanilla_registry() {
        for (name, _) in BIOME_COLORS {
            assert!(BiomeId::named(name).is_some(), "{name}");
        }
    }

    #[test]
    fn an_unknown_biome_draws_as_default_land() {
        let plains = draw(&Flat::new(70, BiomeId::plains()), 1, 1);
        let custom = draw(&Flat::new(70, BiomeId(9_999)), 1, 1);
        assert_eq!(plains, custom);
    }

    #[test]
    fn high_ground_turns_to_rock_then_snow() {
        let green = |pixel: [u8; 4]| pixel[1] > pixel[0] && pixel[1] > pixel[2];
        assert!(green(flat_pixel(SEA + 10, "plains")));
        assert!(!green(flat_pixel(SEA + 64, "plains")));
        let snow = flat_pixel(SEA + 90, "plains");
        assert!(snow[..3].iter().all(|channel| *channel >= 240));
    }

    #[test]
    fn slopes_facing_north_west_are_lit() {
        let ridge = Shaped {
            height: |x: i32, _| 80 - x.abs() / 4,
            biome: |_, _| BiomeId::plains(),
        };
        let image = draw(&ridge, 16, 3);
        assert!(brightness(image.pixel(5, 1)) > brightness(image.pixel(11, 1)));
    }

    #[test]
    fn a_pixel_does_not_depend_on_where_the_view_stands() {
        let generator = DefaultWorldGenerator::default();
        let style = Style::new(SEA);
        let start = window(0, 0, 32, 32, 4);
        let mut moved = start;
        moved.pan(Direction::East);
        moved.pan(Direction::South);
        let before = render(&Terrain::sample(&generator, &start), &style);
        let after = render(&Terrain::sample(&generator, &moved), &style);
        for row in 8..32 {
            for col in 8..32 {
                assert_eq!(before.pixel(col, row), after.pixel(col - 8, row - 8));
            }
        }
    }

    #[test]
    fn the_default_world_shows_land_water_and_relief() {
        let image = draw(&DefaultWorldGenerator::default(), 96, 80);
        let colors: HashSet<_> = image.rgba().chunks(4).collect();
        assert!(colors.len() > 8, "{} colours", colors.len());
        assert!(colors.iter().any(|pixel| pixel[2] > pixel[1]));
        assert!(colors.iter().any(|pixel| pixel[1] > pixel[2]));
    }

    #[test]
    fn the_quantum_rounds_channels() {
        let generator = DefaultWorldGenerator::default();
        let terrain = Terrain::sample(&generator, &window(0, 0, 32, 32, 4));
        let quantum = NonZeroU8::new(32).unwrap();
        let image = render(&terrain, &Style::new(SEA).quantum(quantum));
        for pixel in image.rgba().chunks(4) {
            assert!(pixel[..3].iter().all(|c| c % 32 == 0 || *c == 255));
        }
    }
}
