//! The level the map lives in: black sky, no fog, the same light everywhere.

use ussr_nbt::owned::{Compound, Nbt, Tag};
use voidmc::{BiomeBuilder, BiomeId, RegistryDataStore};
use voidmc_protocol::clientbound::RegistryEntry;

pub const DIMENSION_TYPE_REGISTRY: &str = "minecraft:dimension_type";
pub const DIMENSION: &str = "voidmc:map";
const BIOME: &str = "voidmc:map";

/// Same vertical range as the overworld, which is what `ChunkBuilder` lays out.
const MIN_Y: i32 = -64;
const HEIGHT: i32 = 384;
/// Far past the camera, so that the model never fades out.
const FOG_START: f32 = 2_000.0;
const FOG_END: f32 = 4_000.0;

fn compound<const N: usize>(tags: [(&str, Tag); N]) -> Compound {
    Compound {
        tags: tags
            .into_iter()
            .map(|(name, tag)| (name.into(), tag))
            .collect(),
    }
}

fn string(value: &str) -> Tag {
    Tag::String(value.into())
}

fn dimension_type() -> Nbt {
    // A biome without these attributes leaves them to the dimension.
    let attributes = compound([
        ("minecraft:visual/sky_color", string("#000000")),
        ("minecraft:visual/fog_color", string("#000000")),
        ("minecraft:visual/fog_start_distance", Tag::Float(FOG_START)),
        ("minecraft:visual/fog_end_distance", Tag::Float(FOG_END)),
        ("minecraft:visual/sky_light_color", string("#ffffff")),
        ("minecraft:visual/sky_light_factor", Tag::Float(1.0)),
        ("minecraft:visual/ambient_light_color", string("#ffffff")),
    ]);
    Nbt {
        name: "".into(),
        compound: compound([
            ("ambient_light", Tag::Float(1.0)),
            ("attributes", Tag::Compound(attributes)),
            ("coordinate_scale", Tag::Double(1.0)),
            ("has_ceiling", Tag::Byte(0)),
            ("has_ender_dragon_fight", Tag::Byte(0)),
            ("has_fixed_time", Tag::Byte(1)),
            ("has_skylight", Tag::Byte(1)),
            ("height", Tag::Int(HEIGHT)),
            ("infiniburn", string("#minecraft:infiniburn_overworld")),
            ("logical_height", Tag::Int(HEIGHT)),
            ("min_y", Tag::Int(MIN_Y)),
            ("monster_spawn_block_light_limit", Tag::Int(0)),
            ("monster_spawn_light_level", Tag::Int(0)),
            ("skybox", string("none")),
        ]),
    }
}

/// Where the map sits in the registries that clients receive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapLevel {
    pub dimension_type: i32,
    pub biome: BiomeId,
    pub biome_count: usize,
}

/// Adds the map's dimension type and biome. Must run before any client logs in.
pub fn register(store: &mut RegistryDataStore) -> MapLevel {
    store.add_entry(
        DIMENSION_TYPE_REGISTRY,
        RegistryEntry {
            entry_id: DIMENSION.into(),
            data: Some(dimension_type()),
        },
    );
    let biome = BiomeBuilder::new(BIOME)
        .precipitation(false)
        .temperature(0.8)
        .downfall(0.4)
        .water_color(0x3f76e4)
        .register(store)
        .expect("the map biome registers once");
    let dimension_type = store
        .get_registry(DIMENSION_TYPE_REGISTRY)
        .map_or(0, |registry| registry.entries.len() as i32 - 1);
    MapLevel {
        dimension_type,
        biome,
        biome_count: store.biome_count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_map_comes_after_the_vanilla_dimensions() {
        let mut store = RegistryDataStore::default();
        let vanilla = store
            .get_registry(DIMENSION_TYPE_REGISTRY)
            .unwrap()
            .entries
            .len();
        let level = register(&mut store);
        assert_eq!(level.dimension_type as usize, vanilla);
        let registry = store.get_registry(DIMENSION_TYPE_REGISTRY).unwrap();
        assert_eq!(registry.entries[vanilla].entry_id, DIMENSION);
        assert_eq!(store.biome_name(level.biome), Some(BIOME));
    }

    #[test]
    fn the_height_is_what_the_client_accepts() {
        assert_eq!(HEIGHT % 16, 0);
        assert_eq!(MIN_Y % 16, 0);
    }
}
