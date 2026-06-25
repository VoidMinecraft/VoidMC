//! Configuration for the world persistence plugin.

use std::path::PathBuf;

use bevy_ecs::prelude::Resource;

/// Settings controlling how and where chunks are persisted.
///
/// Threaded into the engine as a [`Resource`] by
/// [`WorldPersistencePlugin`](crate::WorldPersistencePlugin).
#[derive(Resource, Clone, Debug)]
pub struct PersistenceConfig {
    /// Root directory under which `region/<dim>/r.<x>.<z>.vrm` files are written.
    pub directory: PathBuf,
    /// How often (in ticks) the save system flushes dirty chunks to disk.
    pub save_interval_ticks: u64,
    /// Master switch. When `false`, the plugin installs nothing.
    pub enabled: bool,
    /// When `true`, light data is not persisted and is regenerated (full sky)
    /// on load, shrinking files at the cost of fidelity.
    pub regenerate_light_on_load: bool,
    /// Upper bound on chunks written per save tick, to avoid tick spikes.
    /// `0` means unlimited.
    pub max_saves_per_flush: usize,
}

impl Default for PersistenceConfig {
    fn default() -> Self {
        Self {
            directory: PathBuf::from("world"),
            save_interval_ticks: 200,
            enabled: true,
            regenerate_light_on_load: false,
            max_saves_per_flush: 0,
        }
    }
}

impl PersistenceConfig {
    /// Creates a config persisting to `directory` with defaults otherwise.
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
            ..Default::default()
        }
    }

    /// Sets the save interval in ticks.
    pub fn save_interval_ticks(mut self, ticks: u64) -> Self {
        self.save_interval_ticks = ticks;
        self
    }

    /// Enables or disables persistence entirely.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Regenerate light on load instead of persisting it.
    pub fn regenerate_light_on_load(mut self, value: bool) -> Self {
        self.regenerate_light_on_load = value;
        self
    }

    /// Caps chunks written per save tick (`0` = unlimited).
    pub fn max_saves_per_flush(mut self, value: usize) -> Self {
        self.max_saves_per_flush = value;
        self
    }
}
