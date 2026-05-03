use bevy_ecs::prelude::*;
use voidmc_protocol::clientbound::{RegistryData, RegistryEntry};

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
