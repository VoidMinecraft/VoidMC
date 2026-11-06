//! The biome the map is made of. The map stays in the player's dimension, so that going
//! there needs no respawn and no loading screen: this biome alone turns the sky black.

use voidmc::{Attribute, BiomeBuilder, BiomeId, RegistryDataStore};
use voidmc_ui::ResourcePack;

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
/// Out of the view, as far as a level with no clock goes. One with a clock has the sun and
/// the moon from it, whatever the biome says: see [`SKY_BODY_VERTEX`].
const OVERHEAD: f32 = 0.0;
const BLACK: u32 = 0x000000;
const WHITE: u32 = 0xffffff;

const SKY_BODY_VERTEX_FILE: &str = "assets/minecraft/shaders/core/position_tex.vsh";
const SKY_BODY_FRAGMENT_FILE: &str = "assets/minecraft/shaders/core/position_tex.fsh";
/// The shader of the sun and the moon and of nothing else, as the game has it, but that it
/// tells how high they stand. A body under the horizon is hidden by the land of a real
/// world, and the map looks down past the rim of its island: a moon going round under it
/// showed in the void as a grey block.
///
/// The shader is not told where the camera is, or the horizon, but it can work it out. The
/// sky turns about the south-north axis, which is the local X of a body, and a camera never
/// rolls, so that up is square to that axis and to the right of the screen.
const SKY_BODY_VERTEX: &str = r#"#version 330

#moj_import <minecraft:dynamictransforms.glsl>
#moj_import <minecraft:projection.glsl>

in vec3 Position;
in vec2 UV0;

out vec2 texCoord0;
out float overHorizon;

void main() {
    vec4 seen = ModelViewMat * vec4(Position, 1.0);
    gl_Position = ProjMat * seen;

    vec3 axis = ModelViewMat[0].xyz;
    vec3 up = vec3(0.0, axis.z, -axis.y);
    overHorizon = dot(seen.xyz, up * sign(up.y));

    texCoord0 = UV0;
}
"#;
const SKY_BODY_FRAGMENT: &str = r#"#version 330

#moj_import <minecraft:dynamictransforms.glsl>

uniform sampler2D Sampler0;

in vec2 texCoord0;
in float overHorizon;

out vec4 fragColor;

void main() {
    vec4 color = texture(Sampler0, texCoord0);
    if (color.a == 0.0 || overHorizon < 0.0) {
        discard;
    }
    fragColor = color * ColorModulator;
}
"#;

/// Adds what the sky of the map needs of the pack.
pub fn install(pack: &mut ResourcePack) {
    pack.add(SKY_BODY_VERTEX_FILE, SKY_BODY_VERTEX.as_bytes().to_vec());
    pack.add(
        SKY_BODY_FRAGMENT_FILE,
        SKY_BODY_FRAGMENT.as_bytes().to_vec(),
    );
}

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
