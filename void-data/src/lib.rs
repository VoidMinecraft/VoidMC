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

mod entity_types;
use entity_types::*;

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
pub fn entity_type_names(version: Version) -> &'static [&'static str] {
    match version {
        Version::V26_1_2 => ENTITY_TYPE_IDS_26_1_2,
    }
}

/// Returns the protocol numeric ID for a named entity type, or `None` if the
/// name is not in the table for this version.
///
/// The table is populated by running `scripts/gen_entity_types.sh`.
pub fn entity_type_id(version: Version, name: &str) -> Option<i32> {
    let table = match version {
        Version::V26_1_2 => ENTITY_TYPE_IDS_26_1_2,
    };
    table.iter().position(|&n| n == name).map(|i| i as i32)
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
