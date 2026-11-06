use std::sync::atomic::{AtomicBool, Ordering};

use bevy_ecs::prelude::*;
use voidmc_protocol::clientbound::{RegistryData, RegistryEntry};

/// Stores all registry data sent to clients during configuration.
///
/// Use the mutation methods to add, remove, or edit individual registries
/// and entries without replacing the entire set.
#[derive(Resource)]
pub struct RegistryDataStore {
    pub registries: Vec<RegistryData>,
    /// Tags on top of the vanilla ones: registry, tag and the entries in it.
    tags: Vec<(String, String, Vec<String>)>,
    sent: AtomicBool,
}

impl Default for RegistryDataStore {
    fn default() -> Self {
        Self {
            registries: default_registry_data(),
            tags: Vec::new(),
            sent: AtomicBool::new(false),
        }
    }
}

impl RegistryDataStore {
    /// Whether any client has already received these registries; mutations
    /// after that point desynchronise ids (see `BiomeBuilder::register`).
    pub fn sent_to_clients(&self) -> bool {
        self.sent.load(Ordering::Relaxed)
    }

    pub(crate) fn mark_sent(&self) {
        self.sent.store(true, Ordering::Relaxed);
    }

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

    /// Puts entries in a tag, sent to clients along with the vanilla tags. Vanilla reads
    /// some tags of its own, such as `minecraft:pause_screen_additions` of dialogs.
    pub fn add_tag(&mut self, registry_id: &str, tag_id: &str, entry_ids: &[&str]) {
        let entries = entry_ids.iter().map(ToString::to_string).collect();
        self.tags
            .push((registry_id.to_string(), tag_id.to_string(), entries));
    }

    /// The tags added with [`Self::add_tag`].
    pub(crate) fn added_tags(&self) -> impl Iterator<Item = (&str, &str, &[String])> {
        self.tags
            .iter()
            .map(|(registry, tag, entries)| (registry.as_str(), tag.as_str(), entries.as_slice()))
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

/// Returns the default set of registry data needed for a vanilla-compatible server.
///
/// Every shipped registry now comes from `voidmc_data` (datapack JSONs extracted
/// from Paper at the targeted version).
pub fn default_registry_data() -> Vec<RegistryData> {
    let version = voidmc_data::Version::V26_1_2;
    voidmc_data::registries(version)
        .iter()
        .map(|(registry_id, entries)| {
            let entries = entries
                .iter()
                .map(|(entry_id, _bytes)| {
                    let nbt = voidmc_data::entry_nbt(version, registry_id, entry_id)
                        .expect("entry shipped but parse failed");
                    RegistryEntry {
                        entry_id: entry_id.to_string(),
                        data: Some(nbt.clone()),
                    }
                })
                .collect();
            RegistryData {
                registry_id: registry_id.to_string(),
                entries,
            }
        })
        .collect()
}
