//! The biome the map is made of. The map stays in the player's dimension, so that going
//! there needs no respawn and no loading screen: this biome alone turns the sky black.

use voidmc::{Attribute, BiomeBuilder, BiomeId, RegistryDataStore};

use crate::regions::Climate;

const BIOME: &str = "voidmc:map";
const RAIN_BIOME: &str = "voidmc:map_rain";
const SNOW_BIOME: &str = "voidmc:map_snow";
/// Under the temperature where the client turns rain into snow.
const FREEZING: f32 = -0.5;
/// The colours of the mild biome, which a cold one would otherwise turn blue.
const GRASS: u32 = 0x91bd59;
const FOLIAGE: u32 = 0x77ab2f;

/// Far past the camera, so that the model never fades out.
const FOG_START: f32 = 2_000.0;
const FOG_END: f32 = 4_000.0;
/// Out of the view, as far as the sun goes. At night the moon and the stars still show:
/// a level with a clock has them from the clock, whatever the biome says.
const OVERHEAD: f32 = 0.0;
const BLACK: u32 = 0x000000;
const WHITE: u32 = 0xffffff;

/// Where the map's biome sits in the registries that clients receive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapLevel {
    pub biome: BiomeId,
    pub rain: BiomeId,
    pub snow: BiomeId,
    pub biome_count: usize,
}

impl MapLevel {
    /// The biome of a place, which only tells the client what falls there.
    pub fn biome_of(&self, climate: Climate) -> BiomeId {
        match climate {
            Climate::Clear => self.biome,
            Climate::Rain => self.rain,
            Climate::Snow => self.snow,
        }
    }
}

fn map_biome(name: &str) -> BiomeBuilder {
    let visual = |name: &str| format!("minecraft:visual/{name}");
    BiomeBuilder::new(name)
        .temperature(0.8)
        .downfall(0.4)
        .water_color(0x3f76e4)
        .grass_color(GRASS)
        .foliage_color(FOLIAGE)
        .attribute(visual("sky_color"), Attribute::color(BLACK))
        .attribute(visual("fog_color"), Attribute::color(BLACK))
        .attribute(visual("fog_start_distance"), Attribute::float(FOG_START))
        .attribute(visual("fog_end_distance"), Attribute::float(FOG_END))
        .attribute(visual("sky_light_color"), Attribute::color(WHITE))
        .attribute(visual("sky_light_factor"), Attribute::float(1.0))
        .attribute(visual("ambient_light_color"), Attribute::color(WHITE))
        .attribute(visual("star_brightness"), Attribute::float(0.0))
        .attribute(visual("sun_angle"), Attribute::float(OVERHEAD))
        .attribute(visual("moon_angle"), Attribute::float(OVERHEAD))
}

/// Adds the map's biome. Must run before any client logs in.
pub fn register(store: &mut RegistryDataStore) -> MapLevel {
    let biome = map_biome(BIOME)
        .precipitation(false)
        .register(store)
        .expect("the map biome registers once");
    let rain = map_biome(RAIN_BIOME)
        .register(store)
        .expect("the biome of rain registers once");
    let snow = map_biome(SNOW_BIOME)
        .temperature(FREEZING)
        .register(store)
        .expect("the biome of snow registers once");
    MapLevel {
        biome,
        rain,
        snow,
        biome_count: store.biome_count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_biome_comes_after_the_vanilla_ones() {
        let mut store = RegistryDataStore::default();
        let level = register(&mut store);
        assert_eq!(store.biome_name(level.biome), Some(BIOME));
    }

    #[test]
    fn a_climate_has_its_own_biome() {
        let mut store = RegistryDataStore::default();
        let level = register(&mut store);
        assert_eq!(level.biome_of(Climate::Clear), level.biome);
        assert_eq!(
            store.biome_name(level.biome_of(Climate::Rain)),
            Some(RAIN_BIOME)
        );
        assert_eq!(
            store.biome_name(level.biome_of(Climate::Snow)),
            Some(SNOW_BIOME)
        );
    }
}
