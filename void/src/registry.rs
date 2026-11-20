use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

use bevy_ecs::prelude::*;
use voidmc_protocol::clientbound::{RegistryData, RegistryEntry};

/// A registry change refused because a client already received the
/// registries: its ids would no longer match the server's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistriesAlreadySent;

impl fmt::Display for RegistriesAlreadySent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a client already received the registries; change them in \
             ServerConfigBuilder::configure_registries or a plugin, before any client logs in"
        )
    }
}

impl std::error::Error for RegistriesAlreadySent {}

/// Stores all registry data sent to clients during configuration.
///
/// Use the mutation methods to add, remove, or edit individual registries
/// and entries without replacing the entire set. They succeed until the
/// first client receives the registries; from then on each returns
/// [`RegistriesAlreadySent`] and leaves the store unchanged, so every client
/// is sent the same registries. Reading works at any time.
#[derive(Resource)]
pub struct RegistryDataStore {
    registries: Vec<RegistryData>,
    sent: AtomicBool,
}

impl Default for RegistryDataStore {
    fn default() -> Self {
        Self {
            registries: default_registry_data(),
            sent: AtomicBool::new(false),
        }
    }
}

impl RegistryDataStore {
    /// Whether any client has already received these registries; every
    /// mutation is refused with [`RegistriesAlreadySent`] from then on.
    pub fn sent_to_clients(&self) -> bool {
        self.sent.load(Ordering::Relaxed)
    }

    pub(crate) fn mark_sent(&self) {
        self.sent.store(true, Ordering::Relaxed);
    }

    fn ensure_unsent(&self) -> Result<(), RegistriesAlreadySent> {
        if self.sent_to_clients() {
            Err(RegistriesAlreadySent)
        } else {
            Ok(())
        }
    }

    /// Every registry, in the order they are sent.
    pub fn registries(&self) -> &[RegistryData] {
        &self.registries
    }

    /// Look up a registry by its id (e.g. `"minecraft:worldgen/biome"`).
    pub fn get_registry(&self, id: &str) -> Option<&RegistryData> {
        self.registries.iter().find(|r| r.registry_id == id)
    }

    /// Look up a registry mutably by its id.
    pub fn get_registry_mut(
        &mut self,
        id: &str,
    ) -> Result<Option<&mut RegistryData>, RegistriesAlreadySent> {
        self.ensure_unsent()?;
        Ok(self.registries.iter_mut().find(|r| r.registry_id == id))
    }

    /// Append a whole new registry.
    pub fn add_registry(&mut self, registry: RegistryData) -> Result<(), RegistriesAlreadySent> {
        self.ensure_unsent()?;
        self.registries.push(registry);
        Ok(())
    }

    /// Remove a registry by its id, returning it if found.
    pub fn remove_registry(
        &mut self,
        id: &str,
    ) -> Result<Option<RegistryData>, RegistriesAlreadySent> {
        self.ensure_unsent()?;
        let pos = self.registries.iter().position(|r| r.registry_id == id);
        Ok(pos.map(|pos| self.registries.remove(pos)))
    }

    /// Add an entry to an existing registry.
    pub fn add_entry(
        &mut self,
        registry_id: &str,
        entry: RegistryEntry,
    ) -> Result<(), RegistriesAlreadySent> {
        if let Some(reg) = self.get_registry_mut(registry_id)? {
            reg.entries.push(entry);
        }
        Ok(())
    }

    /// Remove an entry from a registry, returning it if found.
    pub fn remove_entry(
        &mut self,
        registry_id: &str,
        entry_id: &str,
    ) -> Result<Option<RegistryEntry>, RegistriesAlreadySent> {
        let Some(reg) = self.get_registry_mut(registry_id)? else {
            return Ok(None);
        };
        let pos = reg.entries.iter().position(|e| e.entry_id == entry_id);
        Ok(pos.map(|pos| reg.entries.remove(pos)))
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
    ) -> Result<Option<&mut RegistryEntry>, RegistriesAlreadySent> {
        let registry = self.get_registry_mut(registry_id)?;
        Ok(registry.and_then(|r| r.entries.iter_mut().find(|e| e.entry_id == entry_id)))
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

#[cfg(test)]
mod tests {
    use ussr_nbt::owned::Nbt;

    use super::*;

    const PAINTINGS: &str = "minecraft:painting_variant";

    type Snapshot = Vec<(String, Vec<(String, Option<Nbt>)>)>;

    fn snapshot(store: &RegistryDataStore) -> Snapshot {
        store
            .registries()
            .iter()
            .map(|registry| {
                let entries = registry
                    .entries
                    .iter()
                    .map(|entry| (entry.entry_id.clone(), entry.data.clone()))
                    .collect();
                (registry.registry_id.clone(), entries)
            })
            .collect()
    }

    /// A default store a client has already received, and what it held then.
    fn sent_store() -> (RegistryDataStore, Snapshot) {
        let store = RegistryDataStore::default();
        store.mark_sent();
        let sent = snapshot(&store);
        (store, sent)
    }

    fn assert_unchanged(store: &RegistryDataStore, sent: &Snapshot) {
        assert!(
            snapshot(store) == *sent,
            "the registries changed after a client received them"
        );
    }

    fn late_entry() -> RegistryEntry {
        RegistryEntry {
            entry_id: "test:late".into(),
            data: None,
        }
    }

    fn late_registry() -> RegistryData {
        RegistryData {
            registry_id: "test:late".into(),
            entries: Vec::new(),
        }
    }

    #[test]
    fn add_entry_is_refused_once_sent() {
        let (mut store, sent) = sent_store();
        assert_eq!(
            store.add_entry(PAINTINGS, late_entry()),
            Err(RegistriesAlreadySent)
        );
        assert_unchanged(&store, &sent);
    }

    #[test]
    fn remove_entry_is_refused_once_sent() {
        let (mut store, sent) = sent_store();
        assert!(matches!(
            store.remove_entry(PAINTINGS, "minecraft:alban"),
            Err(RegistriesAlreadySent)
        ));
        assert_unchanged(&store, &sent);
    }

    #[test]
    fn get_entry_mut_is_refused_once_sent() {
        let (mut store, sent) = sent_store();
        assert!(matches!(
            store.get_entry_mut(PAINTINGS, "minecraft:alban"),
            Err(RegistriesAlreadySent)
        ));
        assert_unchanged(&store, &sent);
    }

    #[test]
    fn add_registry_is_refused_once_sent() {
        let (mut store, sent) = sent_store();
        assert_eq!(
            store.add_registry(late_registry()),
            Err(RegistriesAlreadySent)
        );
        assert_unchanged(&store, &sent);
    }

    #[test]
    fn remove_registry_is_refused_once_sent() {
        let (mut store, sent) = sent_store();
        assert!(matches!(
            store.remove_registry(PAINTINGS),
            Err(RegistriesAlreadySent)
        ));
        assert_unchanged(&store, &sent);
    }

    #[test]
    fn get_registry_mut_is_refused_once_sent() {
        let (mut store, sent) = sent_store();
        assert!(matches!(
            store.get_registry_mut(PAINTINGS),
            Err(RegistriesAlreadySent)
        ));
        assert_unchanged(&store, &sent);
    }

    #[test]
    fn reading_works_once_sent() {
        let (store, sent) = sent_store();
        assert!(store.get_registry(PAINTINGS).is_some());
        assert!(store.get_entry(PAINTINGS, "minecraft:alban").is_some());
        assert_eq!(store.registries().len(), sent.len());
    }

    #[test]
    fn every_mutation_applies_before_registries_are_sent() {
        let mut store = RegistryDataStore::default();
        let count =
            |store: &RegistryDataStore| store.get_registry(PAINTINGS).unwrap().entries.len();
        let paintings = count(&store);

        store.add_entry(PAINTINGS, late_entry()).unwrap();
        assert!(store.get_entry(PAINTINGS, "test:late").is_some());
        let removed = store.remove_entry(PAINTINGS, "minecraft:alban").unwrap();
        assert_eq!(
            removed.map(|entry| entry.entry_id).as_deref(),
            Some("minecraft:alban")
        );
        store
            .get_entry_mut(PAINTINGS, "minecraft:kebab")
            .unwrap()
            .expect("the kebab painting")
            .data = None;
        assert!(
            store
                .get_entry(PAINTINGS, "minecraft:kebab")
                .unwrap()
                .data
                .is_none()
        );
        store
            .get_registry_mut(PAINTINGS)
            .unwrap()
            .unwrap()
            .entries
            .pop();
        assert_eq!(count(&store), paintings - 1);

        store.add_registry(late_registry()).unwrap();
        assert!(store.get_registry("test:late").is_some());
        let removed = store.remove_registry("test:late").unwrap();
        assert_eq!(
            removed.map(|registry| registry.registry_id).as_deref(),
            Some("test:late")
        );
        assert!(store.get_registry("test:late").is_none());
        assert!(!store.sent_to_clients());
    }
}
