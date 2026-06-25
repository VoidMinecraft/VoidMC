//! Vanilla Minecraft registry data, generated at build time from JSON assets
//! shipped under `assets/<version>/`.
//!
//! Each entry is stored as a serialized NBT compound. Use [`registry`] or
//! [`entry_nbt`] to access them; NBT parsing is lazy.
//!
//! Asset extraction is a separate, manual step — see `scripts/extract.sh`.
#![allow(clippy::type_complexity)]

use std::sync::OnceLock;

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
}
