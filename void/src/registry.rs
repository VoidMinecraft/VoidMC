use bevy_ecs::prelude::*;
use ussr_nbt::owned::{Compound, Nbt, Tag};
use void_protocol::clientbound::{RegistryData, RegistryEntry};

/// Stores all registry data sent to clients during configuration.
///
/// Use the mutation methods to add, remove, or edit individual registries
/// and entries without replacing the entire set.
#[derive(Resource)]
pub struct RegistryDataStore {
    pub registries: Vec<RegistryData>,
}

impl Default for RegistryDataStore {
    fn default() -> Self {
        Self {
            registries: default_registry_data(),
        }
    }
}

impl RegistryDataStore {
    /// Look up a registry by its id (e.g. `"minecraft:worldgen/biome"`).
    pub fn get_registry(&self, id: &str) -> Option<&RegistryData> {
        self.registries.iter().find(|r| r.registry_id == id)
    }

    /// Look up a registry mutably by its id.
    pub fn get_registry_mut(&mut self, id: &str) -> Option<&mut RegistryData> {
        self.registries.iter_mut().find(|r| r.registry_id == id)
    }

    /// Append a whole new registry.
    pub fn add_registry(&mut self, registry: RegistryData) {
        self.registries.push(registry);
    }

    /// Remove a registry by its id, returning it if found.
    pub fn remove_registry(&mut self, id: &str) -> Option<RegistryData> {
        let pos = self.registries.iter().position(|r| r.registry_id == id)?;
        Some(self.registries.remove(pos))
    }

    /// Add an entry to an existing registry.
    pub fn add_entry(&mut self, registry_id: &str, entry: RegistryEntry) {
        if let Some(reg) = self.get_registry_mut(registry_id) {
            reg.entries.push(entry);
        }
    }

    /// Remove an entry from a registry, returning it if found.
    pub fn remove_entry(&mut self, registry_id: &str, entry_id: &str) -> Option<RegistryEntry> {
        let reg = self.get_registry_mut(registry_id)?;
        let pos = reg.entries.iter().position(|e| e.entry_id == entry_id)?;
        Some(reg.entries.remove(pos))
    }

    /// Look up a single entry inside a registry.
    pub fn get_entry(&self, registry_id: &str, entry_id: &str) -> Option<&RegistryEntry> {
        self.get_registry(registry_id)?
            .entries
            .iter()
            .find(|e| e.entry_id == entry_id)
    }

    /// Look up a single entry mutably inside a registry.
    pub fn get_entry_mut(
        &mut self,
        registry_id: &str,
        entry_id: &str,
    ) -> Option<&mut RegistryEntry> {
        self.get_registry_mut(registry_id)?
            .entries
            .iter_mut()
            .find(|e| e.entry_id == entry_id)
    }
}

fn nbt(tags: Vec<(&str, Tag)>) -> Nbt {
    Nbt {
        name: Default::default(),
        compound: compound(tags),
    }
}

fn compound(tags: Vec<(&str, Tag)>) -> Compound {
    Compound {
        tags: tags.into_iter().map(|(k, v)| (k.into(), v)).collect(),
    }
}

/// Returns the default set of registry data needed for a vanilla-compatible server.
pub fn default_registry_data() -> Vec<RegistryData> {
    vec![
        // Minimal dimension type registry
        RegistryData {
            registry_id: "minecraft:dimension_type".to_string(),
            entries: vec![RegistryEntry {
                entry_id: "minecraft:overworld".to_string(),
                data: Some(nbt(vec![
                    ("has_skylight", Tag::Byte(1)),
                    ("has_ceiling", Tag::Byte(0)),
                    ("ultrawarm", Tag::Byte(0)),
                    ("natural", Tag::Byte(1)),
                    ("coordinate_scale", Tag::Double(1.0)),
                    ("bed_works", Tag::Byte(1)),
                    ("respawn_anchor_works", Tag::Byte(0)),
                    ("min_y", Tag::Int(-64)),
                    ("height", Tag::Int(384)),
                    ("logical_height", Tag::Int(384)),
                    (
                        "infiniburn",
                        Tag::String("#minecraft:infiniburn_overworld".into()),
                    ),
                    ("effects", Tag::String("minecraft:overworld".into())),
                    ("ambient_light", Tag::Float(0.0)),
                    ("piglin_safe", Tag::Byte(0)),
                    ("has_raids", Tag::Byte(1)),
                    ("monster_spawn_light_level", Tag::Int(0)),
                    ("monster_spawn_block_light_limit", Tag::Int(0)),
                ])),
            }],
        },
        // Minimal biome registry
        RegistryData {
            registry_id: "minecraft:worldgen/biome".to_string(),
            entries: vec![RegistryEntry {
                entry_id: "minecraft:plains".to_string(),
                data: Some(nbt(vec![
                    ("has_precipitation", Tag::Byte(1)),
                    ("temperature", Tag::Float(0.8)),
                    ("downfall", Tag::Float(0.4)),
                    (
                        "effects",
                        Tag::Compound(compound(vec![
                            ("sky_color", Tag::Int(7907327)),
                            ("water_fog_color", Tag::Int(329011)),
                            ("fog_color", Tag::Int(12638463)),
                            ("water_color", Tag::Int(4159204)),
                            ("grass_color", Tag::Int(7979098)),
                            ("foliage_color", Tag::Int(6208527)),
                            (
                                "mood_sound",
                                Tag::Compound(compound(vec![
                                    ("sound", Tag::String("minecraft:ambient.cave".into())),
                                    ("tick_delay", Tag::Int(6000)),
                                    ("block_search_extent", Tag::Int(8)),
                                    ("offset", Tag::Double(2.0)),
                                ])),
                            ),
                        ])),
                    ),
                ])),
            }],
        },
        // Painting variant registry (required)
        RegistryData {
            registry_id: "minecraft:painting_variant".to_string(),
            entries: vec![RegistryEntry {
                entry_id: "minecraft:kebab".to_string(),
                data: Some(nbt(vec![
                    ("asset_id", Tag::String("minecraft:kebab".into())),
                    ("width", Tag::Int(1)),
                    ("height", Tag::Int(1)),
                ])),
            }],
        },
        // Damage type registry (required — all 25 mandatory types)
        RegistryData {
            registry_id: "minecraft:damage_type".to_string(),
            entries: [
                ("minecraft:generic", "generic", "never"),
                ("minecraft:generic_kill", "genericKill", "never"),
                ("minecraft:fall", "fall", "when_caused_by_living_non_player"),
                ("minecraft:in_fire", "inFire", "never"),
                ("minecraft:on_fire", "onFire", "never"),
                ("minecraft:lava", "lava", "never"),
                ("minecraft:hot_floor", "hotFloor", "never"),
                ("minecraft:in_wall", "inWall", "never"),
                ("minecraft:cramming", "cramming", "never"),
                ("minecraft:drown", "drown", "never"),
                ("minecraft:starve", "starve", "never"),
                ("minecraft:cactus", "cactus", "never"),
                ("minecraft:sweet_berry_bush", "sweetBerryBush", "never"),
                ("minecraft:freeze", "freeze", "never"),
                ("minecraft:fly_into_wall", "flyIntoWall", "never"),
                ("minecraft:out_of_world", "outOfWorld", "never"),
                ("minecraft:magic", "magic", "never"),
                ("minecraft:wither", "wither", "never"),
                ("minecraft:dragon_breath", "dragonBreath", "never"),
                ("minecraft:dry_out", "dryout", "never"),
                ("minecraft:campfire", "inFire", "never"),
                ("minecraft:lightning_bolt", "lightningBolt", "never"),
                ("minecraft:stalagmite", "stalagmite", "never"),
                ("minecraft:outside_border", "outsideBorder", "never"),
                ("minecraft:ender_pearl", "fall", "never"),
            ]
            .into_iter()
            .map(|(id, msg, scaling)| RegistryEntry {
                entry_id: id.to_string(),
                data: Some(nbt(vec![
                    ("message_id", Tag::String(msg.into())),
                    ("scaling", Tag::String(scaling.into())),
                    ("exhaustion", Tag::Float(0.0)),
                ])),
            })
            .collect(),
        },
        // Wolf variant registry (required non-empty)
        RegistryData {
            registry_id: "minecraft:wolf_variant".to_string(),
            entries: vec![RegistryEntry {
                entry_id: "minecraft:pale".to_string(),
                data: Some(nbt(vec![
                    (
                        "wild_texture",
                        Tag::String("minecraft:textures/entity/wolf/wolf.png".into()),
                    ),
                    (
                        "tame_texture",
                        Tag::String("minecraft:textures/entity/wolf/wolf_tame.png".into()),
                    ),
                    (
                        "angry_texture",
                        Tag::String("minecraft:textures/entity/wolf/wolf_angry.png".into()),
                    ),
                    ("biomes", Tag::String("minecraft:plains".into())),
                ])),
            }],
        },
        // Wolf sound variant registry (required non-empty)
        RegistryData {
            registry_id: "minecraft:wolf_sound_variant".to_string(),
            entries: vec![RegistryEntry {
                entry_id: "minecraft:classic".to_string(),
                data: Some(nbt(vec![
                    (
                        "ambient_sound",
                        Tag::String("minecraft:entity.wolf.ambient".into()),
                    ),
                    (
                        "death_sound",
                        Tag::String("minecraft:entity.wolf.death".into()),
                    ),
                    (
                        "growl_sound",
                        Tag::String("minecraft:entity.wolf.growl".into()),
                    ),
                    (
                        "hurt_sound",
                        Tag::String("minecraft:entity.wolf.hurt".into()),
                    ),
                    (
                        "pant_sound",
                        Tag::String("minecraft:entity.wolf.pant".into()),
                    ),
                    (
                        "whine_sound",
                        Tag::String("minecraft:entity.wolf.whine".into()),
                    ),
                ])),
            }],
        },
        // Cat variant registry (required non-empty)
        RegistryData {
            registry_id: "minecraft:cat_variant".to_string(),
            entries: vec![RegistryEntry {
                entry_id: "minecraft:tabby".to_string(),
                data: Some(nbt(vec![(
                    "texture",
                    Tag::String("minecraft:textures/entity/cat/tabby.png".into()),
                )])),
            }],
        },
        // Chicken variant registry (required non-empty)
        RegistryData {
            registry_id: "minecraft:chicken_variant".to_string(),
            entries: vec![RegistryEntry {
                entry_id: "minecraft:temperate".to_string(),
                data: Some(nbt(vec![(
                    "texture",
                    Tag::String("minecraft:textures/entity/chicken/temperate.png".into()),
                )])),
            }],
        },
        // Cow variant registry (required non-empty)
        RegistryData {
            registry_id: "minecraft:cow_variant".to_string(),
            entries: vec![RegistryEntry {
                entry_id: "minecraft:temperate".to_string(),
                data: Some(nbt(vec![(
                    "texture",
                    Tag::String("minecraft:textures/entity/cow/temperate.png".into()),
                )])),
            }],
        },
        // Frog variant registry (required non-empty)
        RegistryData {
            registry_id: "minecraft:frog_variant".to_string(),
            entries: vec![RegistryEntry {
                entry_id: "minecraft:temperate".to_string(),
                data: Some(nbt(vec![(
                    "texture",
                    Tag::String("minecraft:textures/entity/frog/temperate.png".into()),
                )])),
            }],
        },
        // Pig variant registry (required non-empty)
        RegistryData {
            registry_id: "minecraft:pig_variant".to_string(),
            entries: vec![RegistryEntry {
                entry_id: "minecraft:temperate".to_string(),
                data: Some(nbt(vec![(
                    "texture",
                    Tag::String("minecraft:textures/entity/pig/temperate.png".into()),
                )])),
            }],
        },
    ]
}
