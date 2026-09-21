//! Vanilla Minecraft registry data, generated at build time from JSON assets
//! shipped under `assets/<version>/`.
//!
//! Each entry is stored as a serialized NBT compound. Use [`registry`] or
//! [`entry_nbt`] to access them; NBT parsing is lazy.
//!
//! Asset extraction is a separate, manual step — see `scripts/extract.sh`.
#![allow(clippy::type_complexity)]

use std::sync::OnceLock;

mod biome_attributes;
pub use biome_attributes::*;

use ussr_nbt::owned::Nbt;

include!(concat!(env!("OUT_DIR"), "/registries.rs"));
include!(concat!(env!("OUT_DIR"), "/blocks.rs"));

mod stack_sizes;

/// A supported Minecraft version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    V26_1_2,
}

impl Version {
    fn id(self) -> &'static str {
        match self {
            Version::V26_1_2 => "26.1.2",
        }
    }
}

/// Returns every entity type name for `version` in protocol-ID order.
pub fn entity_type_names(version: Version) -> Vec<&'static str> {
    protocol_registry(version, "minecraft:entity_type")
        .map(|entries| entries.iter().map(|(name, _)| *name).collect())
        .unwrap_or_default()
}

/// Returns the protocol numeric ID for a named entity type, or `None` if the
/// name is not in the table for this version.
pub fn entity_type_id(version: Version, name: &str) -> Option<i32> {
    protocol_registry_index(version, "minecraft:entity_type", name)
}

/// Returns whether `name` is a valid runtime target for `/summon`.
pub fn is_summonable_entity_type(version: Version, name: &str) -> bool {
    entity_type_id(version, name).is_some() && !non_summonable_entity_types(version).contains(&name)
}

/// Returns the raw `(entry_id, protocol_id)` slice for protocol registry IDs
/// from Mojang's `reports/registries.json` data generator output.
pub fn protocol_registry(
    version: Version,
    registry_id: &str,
) -> Option<&'static [(&'static str, i32)]> {
    let (_, registries) = PROTOCOL_REGISTRIES
        .iter()
        .find(|(v, _)| *v == version.id())?;
    let (_, entries) = registries.iter().find(|(id, _)| *id == registry_id)?;
    Some(*entries)
}

/// Returns the protocol numeric ID of `entry_id` in `registry_id` for
/// `version`, or `None` if it is not present in `reports/registries.json`.
pub fn protocol_registry_index(version: Version, registry_id: &str, entry_id: &str) -> Option<i32> {
    protocol_registry(version, registry_id)?
        .iter()
        .find(|(id, _)| *id == entry_id)
        .map(|(_, protocol_id)| *protocol_id)
}

pub fn particle_type_id(version: Version, name: &str) -> Option<i32> {
    protocol_registry_index(version, "minecraft:particle_type", name)
}

pub fn sound_event_id(version: Version, name: &str) -> Option<i32> {
    protocol_registry_index(version, "minecraft:sound_event", name)
}

pub fn block_entity_type_id(version: Version, name: &str) -> Option<i32> {
    protocol_registry_index(version, "minecraft:block_entity_type", name)
}

pub fn block_entity_type_name(version: Version, id: i32) -> Option<&'static str> {
    protocol_registry(version, "minecraft:block_entity_type")?
        .iter()
        .find(|(_, protocol_id)| *protocol_id == id)
        .map(|(name, _)| *name)
}

/// The block entity type hosted by a block state, or `None` when that block
/// carries no block entity.
pub fn block_entity_type_for_state(version: Version, block_state_id: i32) -> Option<&'static str> {
    let table = match version {
        Version::V26_1_2 => v26_1_2::block_entities::HOSTS,
    };
    let idx = table.partition_point(|(min, _, _)| *min <= block_state_id);
    let (min, max, kind) = table.get(idx.checked_sub(1)?)?;
    (*min..=*max).contains(&block_state_id).then_some(*kind)
}

pub fn menu_id(version: Version, name: &str) -> Option<i32> {
    protocol_registry_index(version, "minecraft:menu", name)
}

pub fn data_component_type_id(version: Version, name: &str) -> Option<i32> {
    protocol_registry_index(version, "minecraft:data_component_type", name)
}

pub fn protocol_version(version: Version) -> i32 {
    VERSION_INFO
        .iter()
        .find(|(v, _, _)| *v == version.id())
        .map(|(_, protocol, _)| *protocol)
        .expect("every Version has a version.json asset")
}

pub fn world_version(version: Version) -> i32 {
    VERSION_INFO
        .iter()
        .find(|(v, _, _)| *v == version.id())
        .map(|(_, _, world)| *world)
        .expect("every Version has a version.json asset")
}

/// `state`: `handshake` | `status` | `login` | `configuration` | `play`;
/// `direction`: `clientbound` | `serverbound`. Sorted by id.
pub fn packets(
    version: Version,
    state: &str,
    direction: &str,
) -> Option<&'static [(&'static str, i32)]> {
    let (_, states) = PACKETS.iter().find(|(v, _)| *v == version.id())?;
    let (_, directions) = states.iter().find(|(s, _)| *s == state)?;
    let (_, entries) = directions.iter().find(|(d, _)| *d == direction)?;
    Some(*entries)
}

pub fn packet_id(version: Version, state: &str, direction: &str, name: &str) -> Option<i32> {
    packets(version, state, direction)?
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, id)| *id)
}

/// Returns the protocol item id for a full item id like `"minecraft:stone"`,
/// or `None` if the name is not in the `minecraft:item` registry for `version`.
pub fn item_id(version: Version, name: &str) -> Option<i32> {
    let table = match version {
        Version::V26_1_2 => v26_1_2::items::ITEM_IDS,
    };
    table
        .binary_search_by(|(n, _)| (*n).cmp(name))
        .ok()
        .map(|i| table[i].1)
}

/// Returns the full item id (e.g. `"minecraft:stone"`) for a protocol item id,
/// or `None` if the id is not in the `minecraft:item` registry for `version`.
pub fn item_name(version: Version, id: i32) -> Option<&'static str> {
    let table = match version {
        Version::V26_1_2 => v26_1_2::items::ITEM_IDS,
    };
    table.iter().find(|(_, i)| *i == id).map(|(n, _)| *n)
}

/// Returns the default block-state id placed by a block item, or `None` if the
/// item does not correspond to a placeable block.
pub fn item_default_block_state(version: Version, item_id: i32) -> Option<i32> {
    let table = match version {
        Version::V26_1_2 => v26_1_2::items::ITEM_TO_BLOCK_STATE,
    };
    table
        .binary_search_by(|(i, _)| i.cmp(&item_id))
        .ok()
        .map(|i| table[i].1)
}

/// Returns every item id name (e.g. `"minecraft:stone"`) for `version`, useful
/// for command tab-completion.
pub fn item_names(version: Version) -> Vec<&'static str> {
    let table = match version {
        Version::V26_1_2 => v26_1_2::items::ITEM_IDS,
    };
    table.iter().map(|(n, _)| *n).collect()
}

/// Returns the maximum stack size for an item id (defaults to 64 for the vast
/// majority of items; tools/armor are 1, a handful of items are 16).
pub fn item_max_stack(version: Version, item_id: i32) -> u8 {
    let table = match version {
        Version::V26_1_2 => stack_sizes::ITEM_STACK_SIZES,
    };
    table
        .binary_search_by(|(id, _)| id.cmp(&item_id))
        .map(|i| table[i].1)
        .unwrap_or(64)
}

/// Returns entity types known by the versioned data to be excluded from
/// Minecraft's `minecraft:summonable_entities` suggestion provider.
pub fn non_summonable_entity_types(version: Version) -> &'static [&'static str] {
    NON_SUMMONABLE_ENTITY_TYPES
        .iter()
        .find(|(v, _)| *v == version.id())
        .map(|(_, entity_types)| *entity_types)
        .unwrap_or(&[])
}

/// Returns the raw `(entry_id, nbt_bytes)` slice for `(version, registry_id)`,
/// or `None` if the registry is not shipped for this version.
pub fn registry(
    version: Version,
    registry_id: &str,
) -> Option<&'static [(&'static str, &'static [u8])]> {
    let (_, registries) = REGISTRIES.iter().find(|(v, _)| *v == version.id())?;
    let (_, entries) = registries.iter().find(|(id, _)| *id == registry_id)?;
    Some(*entries)
}

/// Returns the index of `entry_id` within `registry_id` for `version`, which
/// is the numeric ID the client uses on the wire (e.g. in chunk biome
/// palettes, dimension types in the Login packet). Returns `None` if the
/// registry or entry isn't shipped.
pub fn registry_index(version: Version, registry_id: &str, entry_id: &str) -> Option<i32> {
    let entries = registry(version, registry_id)?;
    entries
        .iter()
        .position(|(id, _)| *id == entry_id)
        .map(|i| i as i32)
}

/// Returns every registry shipped for `version`.
pub fn registries(
    version: Version,
) -> &'static [(&'static str, &'static [(&'static str, &'static [u8])])] {
    REGISTRIES
        .iter()
        .find(|(v, _)| *v == version.id())
        .map(|(_, regs)| *regs)
        .unwrap_or(&[])
}

/// Returns `[(tag_id, [entry_id, ...]), ...]` for `(version, registry_id)`.
/// All `#tag` references are pre-resolved to direct entry IDs at build time.
pub fn tags(
    version: Version,
    registry_id: &str,
) -> &'static [(&'static str, &'static [&'static str])] {
    TAGS.iter()
        .find(|(v, _)| *v == version.id())
        .and_then(|(_, regs)| regs.iter().find(|(id, _)| *id == registry_id))
        .map(|(_, t)| *t)
        .unwrap_or(&[])
}

/// Returns every tagged registry shipped for `version`.
pub fn tagged_registries(
    version: Version,
) -> &'static [(
    &'static str,
    &'static [(&'static str, &'static [&'static str])],
)] {
    TAGS.iter()
        .find(|(v, _)| *v == version.id())
        .map(|(_, regs)| *regs)
        .unwrap_or(&[])
}

/// Lazily parses a single entry's NBT. Each call after the first returns a
/// cached reference.
pub fn entry_nbt(version: Version, registry_id: &str, entry_id: &str) -> Option<&'static Nbt> {
    let entries = registry(version, registry_id)?;
    let (_, bytes) = entries.iter().find(|(id, _)| *id == entry_id)?;
    Some(parse_cached(bytes))
}

fn parse_cached(bytes: &'static [u8]) -> &'static Nbt {
    use std::collections::HashMap;
    use std::sync::Mutex;

    static CACHE: OnceLock<Mutex<HashMap<usize, &'static Nbt>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = bytes.as_ptr() as usize;

    {
        let guard = cache.lock().unwrap();
        if let Some(nbt) = guard.get(&key) {
            return nbt;
        }
    }

    let mut cursor = bytes;
    let parsed = Nbt::read(&mut cursor).expect("embedded NBT parse failed");
    let leaked: &'static Nbt = Box::leak(Box::new(parsed));

    let mut guard = cache.lock().unwrap();
    guard.entry(key).or_insert(leaked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_type_protocol_ids_come_from_protocol_registry_data() {
        assert_eq!(
            entity_type_id(Version::V26_1_2, "minecraft:creeper"),
            Some(32)
        );
        assert_eq!(
            entity_type_id(Version::V26_1_2, "minecraft:zombie"),
            Some(150)
        );
        assert_eq!(
            entity_type_id(Version::V26_1_2, "minecraft:player"),
            Some(155)
        );
        assert_eq!(entity_type_id(Version::V26_1_2, "minecraft:not_real"), None);
    }

    #[test]
    fn hardcoded_registries_are_all_shipped() {
        let v = Version::V26_1_2;
        assert_eq!(particle_type_id(v, "minecraft:dust"), Some(14));
        assert_eq!(particle_type_id(v, "minecraft:not_real"), None);
        assert_eq!(
            sound_event_id(v, "minecraft:entity.player.levelup"),
            Some(1313)
        );
        assert_eq!(block_entity_type_id(v, "minecraft:chest"), Some(1));
        assert_eq!(block_entity_type_name(v, 1), Some("minecraft:chest"));
        assert_eq!(block_entity_type_name(v, 4096), None);
        assert_eq!(menu_id(v, "minecraft:generic_9x3"), Some(2));
        assert_eq!(data_component_type_id(v, "minecraft:custom_data"), Some(0));
        assert_eq!(
            protocol_registry_index(v, "minecraft:entity_type", "minecraft:creeper"),
            Some(32)
        );
        assert!(
            protocol_registry(v, "minecraft:sound_event").unwrap().len() > 1500,
            "sound_event registry looks truncated"
        );
    }

    #[test]
    fn version_info_comes_from_server_jar() {
        assert_eq!(protocol_version(Version::V26_1_2), 775);
        assert_eq!(world_version(Version::V26_1_2), 4790);
    }

    #[test]
    fn packet_ids_come_from_mojang_report() {
        let v = Version::V26_1_2;
        assert_eq!(
            packet_id(v, "play", "clientbound", "minecraft:bundle_delimiter"),
            Some(0)
        );
        assert_eq!(
            packet_id(v, "play", "clientbound", "minecraft:add_entity"),
            Some(1)
        );
        assert_eq!(
            packet_id(v, "play", "serverbound", "minecraft:accept_teleportation"),
            Some(0)
        );
        assert_eq!(
            packet_id(v, "handshake", "serverbound", "minecraft:intention"),
            Some(0)
        );
        assert_eq!(
            packet_id(v, "handshake", "clientbound", "minecraft:intention"),
            None
        );
        assert_eq!(
            packet_id(v, "status", "clientbound", "minecraft:status_response"),
            Some(0)
        );
        assert_eq!(
            packet_id(v, "login", "clientbound", "minecraft:login_finished"),
            Some(2)
        );
        assert_eq!(
            packet_id(v, "configuration", "clientbound", "minecraft:registry_data"),
            Some(7)
        );
        assert_eq!(v26_1_2::packets::play::clientbound::ADD_ENTITY, 1);
        assert_eq!(v26_1_2::packets::play::clientbound::DEBUG_BLOCK_VALUE, 26);
        for (state, dirs) in [
            ("handshake", &["serverbound"][..]),
            ("status", &["clientbound", "serverbound"]),
            ("login", &["clientbound", "serverbound"]),
            ("configuration", &["clientbound", "serverbound"]),
            ("play", &["clientbound", "serverbound"]),
        ] {
            for dir in dirs {
                let table =
                    packets(v, state, dir).unwrap_or_else(|| panic!("{state}/{dir} missing"));
                assert!(!table.is_empty(), "{state}/{dir} is empty");
                for (i, (_, id)) in table.iter().enumerate() {
                    assert_eq!(*id, i as i32, "{state}/{dir} ids are not dense");
                }
            }
        }
    }

    #[test]
    fn biome_nbt_contains_exactly_network_fields() {
        const NETWORK_FIELDS: &[&str] = &[
            "has_precipitation",
            "temperature",
            "temperature_modifier",
            "downfall",
            "attributes",
            "effects",
        ];
        const REQUIRED: &[&str] = &["has_precipitation", "temperature", "downfall", "effects"];
        let biomes = registry(Version::V26_1_2, "minecraft:worldgen/biome").unwrap();
        assert!(biomes.len() >= 60, "expected the vanilla biome set");
        for (id, _) in biomes {
            let nbt = entry_nbt(Version::V26_1_2, "minecraft:worldgen/biome", id).unwrap();
            let keys: Vec<String> = nbt
                .compound
                .tags
                .iter()
                .map(|(k, _)| k.to_string())
                .collect();
            for key in &keys {
                assert!(
                    NETWORK_FIELDS.contains(&key.as_str()),
                    "{id}: non-network field {key:?} shipped"
                );
            }
            for req in REQUIRED {
                assert!(
                    keys.iter().any(|k| k == req),
                    "{id}: required field {req:?} missing"
                );
            }
            if let Some((_, ussr_nbt::owned::Tag::Compound(attrs))) = nbt
                .compound
                .tags
                .iter()
                .find(|(k, _)| k.to_string() == "attributes")
            {
                for (key, _) in &attrs.tags {
                    let key = key.to_string();
                    assert!(
                        !key.starts_with("minecraft:gameplay/")
                            || [
                                "minecraft:gameplay/sky_light_level",
                                "minecraft:gameplay/water_evaporates",
                                "minecraft:gameplay/fast_lava",
                                "minecraft:gameplay/piglins_zombify",
                                "minecraft:gameplay/creaking_active",
                            ]
                            .contains(&key.as_str()),
                        "{id}: non-syncable attribute {key:?} shipped"
                    );
                }
            }
        }
        assert!(
            registry_index(
                Version::V26_1_2,
                "minecraft:worldgen/biome",
                "minecraft:plains"
            )
            .is_some(),
            "minecraft:plains must be in the synced biome registry (client rejects login otherwise)"
        );
    }

    #[test]
    fn entity_kind_enum_matches_registry() {
        use v26_1_2::EntityKind;
        assert_eq!(EntityKind::Zombie.id(), 150);
        assert_eq!(EntityKind::Zombie.name(), "minecraft:zombie");
        assert_eq!(
            EntityKind::from_name("minecraft:pig"),
            Some(EntityKind::Pig)
        );
        assert_eq!(EntityKind::from_id(150), Some(EntityKind::Zombie));
        assert_eq!(EntityKind::from_name("minecraft:nope"), None);
        for kind in EntityKind::ALL {
            assert_eq!(
                entity_type_id(Version::V26_1_2, kind.name()),
                Some(kind.id())
            );
        }
        assert_eq!(
            EntityKind::ALL.len(),
            entity_type_names(Version::V26_1_2).len()
        );
    }

    #[test]
    fn effect_enum_matches_registry_and_paper_table() {
        use v26_1_2::{Effect, EffectCategory};
        let registry = protocol_registry(Version::V26_1_2, "minecraft:mob_effect").unwrap();
        assert_eq!(Effect::ALL.len(), registry.len());
        for effect in Effect::ALL {
            assert_eq!(
                protocol_registry_index(Version::V26_1_2, "minecraft:mob_effect", effect.name()),
                Some(effect.id())
            );
            assert_eq!(Effect::from_id(effect.id()), Some(*effect));
            assert_eq!(Effect::from_name(effect.name()), Some(*effect));
        }
        assert_eq!(Effect::Speed.id(), 0);
        assert_eq!(Effect::Speed.color(), 3402751);
        assert_eq!(Effect::Speed.category(), EffectCategory::Beneficial);
        assert_eq!(Effect::Slowness.category(), EffectCategory::Harmful);
        assert_eq!(Effect::Glowing.category(), EffectCategory::Neutral);
        assert_eq!(Effect::Glowing.color(), 9740385);
        assert_eq!(Effect::Speed.particle(), None);
        assert_eq!(Effect::TrialOmen.particle(), Some("minecraft:trial_omen"));
        assert_eq!(Effect::RaidOmen.particle(), Some("minecraft:raid_omen"));
    }

    #[test]
    fn entity_attribute_enum_matches_registry_and_paper_table() {
        use v26_1_2::EntityAttribute;
        let registry = protocol_registry(Version::V26_1_2, "minecraft:attribute").unwrap();
        assert_eq!(EntityAttribute::ALL.len(), registry.len());
        for attribute in EntityAttribute::ALL {
            assert_eq!(
                protocol_registry_index(Version::V26_1_2, "minecraft:attribute", attribute.name()),
                Some(attribute.id())
            );
            let (min, max) = attribute.range();
            assert!(min <= attribute.default_value() && attribute.default_value() <= max);
        }
        assert_eq!(EntityAttribute::MovementSpeed.default_value(), 0.7);
        assert_eq!(EntityAttribute::MaxHealth.range(), (1.0, 1024.0));
        assert_eq!(EntityAttribute::Scale.range(), (0.0625, 16.0));
        assert!(EntityAttribute::MovementSpeed.is_client_syncable());
        assert!(EntityAttribute::Scale.is_client_syncable());
        assert!(EntityAttribute::Gravity.is_client_syncable());
        assert!(!EntityAttribute::AttackDamage.is_client_syncable());
        assert!(!EntityAttribute::FollowRange.is_client_syncable());
        assert!(!EntityAttribute::WaypointTransmitRange.is_client_syncable());
        assert_eq!(
            EntityAttribute::ALL
                .iter()
                .filter(|a| a.is_client_syncable())
                .count(),
            27
        );
    }

    #[test]
    fn item_ids_resolve_both_directions() {
        assert_eq!(item_id(Version::V26_1_2, "minecraft:air"), Some(0));
        assert_eq!(item_id(Version::V26_1_2, "minecraft:stone"), Some(1));
        assert_eq!(item_id(Version::V26_1_2, "minecraft:not_real"), None);
        assert_eq!(item_name(Version::V26_1_2, 1), Some("minecraft:stone"));
        assert_eq!(item_name(Version::V26_1_2, -1), None);
    }

    #[test]
    fn item_max_stack_sizes() {
        // Default for most items.
        let stone = item_id(Version::V26_1_2, "minecraft:stone").unwrap();
        assert_eq!(item_max_stack(Version::V26_1_2, stone), 64);
        // Tools are unstackable.
        let sword = item_id(Version::V26_1_2, "minecraft:diamond_sword").unwrap();
        assert_eq!(item_max_stack(Version::V26_1_2, sword), 1);
        // Ender pearls stack to 16.
        let pearl = item_id(Version::V26_1_2, "minecraft:ender_pearl").unwrap();
        assert_eq!(item_max_stack(Version::V26_1_2, pearl), 16);
    }

    #[test]
    fn block_items_map_to_their_default_state() {
        // The stone item places the stone block (default state id 1).
        let stone_item = item_id(Version::V26_1_2, "minecraft:stone").unwrap();
        assert_eq!(
            item_default_block_state(Version::V26_1_2, stone_item),
            Some(v26_1_2::blocks::STONE)
        );
        // A non-block item (a tool) has no block state.
        let sword = item_id(Version::V26_1_2, "minecraft:diamond_sword").unwrap();
        assert_eq!(item_default_block_state(Version::V26_1_2, sword), None);
    }

    #[test]
    fn summonable_entity_validation_rejects_special_runtime_types() {
        assert!(is_summonable_entity_type(
            Version::V26_1_2,
            "minecraft:zombie"
        ));
        assert!(!is_summonable_entity_type(
            Version::V26_1_2,
            "minecraft:player"
        ));
        assert!(!is_summonable_entity_type(
            Version::V26_1_2,
            "minecraft:fishing_bobber"
        ));
        assert!(!is_summonable_entity_type(
            Version::V26_1_2,
            "minecraft:not_real"
        ));
    }

    #[test]
    fn block_entity_hosts_come_from_block_state_ranges() {
        let v = Version::V26_1_2;
        assert_eq!(
            block_entity_type_for_state(v, v26_1_2::blocks::OAK_SIGN),
            Some("minecraft:sign")
        );
        assert_eq!(
            block_entity_type_for_state(v, v26_1_2::state::OakWallSign::MAX_STATE_ID),
            Some("minecraft:sign")
        );
        assert_eq!(
            block_entity_type_for_state(v, v26_1_2::blocks::OAK_HANGING_SIGN),
            Some("minecraft:hanging_sign")
        );
        assert_eq!(
            block_entity_type_for_state(v, v26_1_2::blocks::PLAYER_HEAD),
            Some("minecraft:skull")
        );
        assert_eq!(
            block_entity_type_for_state(v, v26_1_2::blocks::RED_WALL_BANNER),
            Some("minecraft:banner")
        );
        assert_eq!(
            block_entity_type_for_state(v, v26_1_2::blocks::CHEST),
            Some("minecraft:chest")
        );
        assert_eq!(block_entity_type_for_state(v, v26_1_2::blocks::STONE), None);
        assert_eq!(block_entity_type_for_state(v, -1), None);
        assert_eq!(block_entity_type_for_state(v, i32::MAX), None);
        for (_, _, kind) in v26_1_2::block_entities::HOSTS {
            assert!(
                block_entity_type_id(v, kind).is_some(),
                "{kind} not in registry"
            );
        }
        assert_eq!(v26_1_2::block_entities::HOSTS.len(), 201);
        for pair in v26_1_2::block_entities::HOSTS.windows(2) {
            assert!(pair[0].1 < pair[1].0, "host ranges overlap: {pair:?}");
        }
    }
}
