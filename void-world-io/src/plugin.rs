//! The Bevy plugin that wires persistence into a VoidMC server.

use std::sync::Arc;

use bevy_app::{App, Last, Plugin, PostUpdate};
use voidmc::ChunkLoaderResource;

use crate::config::PersistenceConfig;
use crate::region::RegionChunkStore;
use crate::store::{ChunkStore, ChunkStoreResource, StoreLoader};
use crate::systems::{SaveTicker, flush_on_exit, save_dirty_chunks};

/// Adds chunk disk persistence: loads chunks from disk before generation and
/// periodically saves modified chunks. Install via `add_plugin` on `VoidServer`.
///
/// ```no_run
/// use voidmc::{ServerConfigBuilder, VoidServer};
/// use voidmc_world_io::{PersistenceConfig, WorldPersistencePlugin};
///
/// VoidServer::new(ServerConfigBuilder::new().build())
///     .add_plugin(|app| {
///         app.add_plugins(WorldPersistencePlugin::new(
///             PersistenceConfig::new("world"),
///         ));
///     })
///     .run();
/// ```
pub struct WorldPersistencePlugin {
    config: PersistenceConfig,
    store: Option<Arc<dyn ChunkStore>>,
}

impl WorldPersistencePlugin {
    /// Creates the plugin using the default region-file store at
    /// `config.directory`.
    pub fn new(config: PersistenceConfig) -> Self {
        Self {
            config,
            store: None,
        }
    }

    /// Creates the plugin with a custom [`ChunkStore`] backend.
    pub fn with_store(config: PersistenceConfig, store: Arc<dyn ChunkStore>) -> Self {
        Self {
            config,
            store: Some(store),
        }
    }
}

impl Plugin for WorldPersistencePlugin {
    fn build(&self, app: &mut App) {
        if !self.config.enabled {
            return;
        }

        let store: Arc<dyn ChunkStore> = self
            .store
            .clone()
            .unwrap_or_else(|| Arc::new(RegionChunkStore::new(self.config.directory.clone())));

        // Graceful shutdown (Ctrl-C, SIGTERM, or `/stop`) is signalled via
        // `AppExit`; `flush_on_exit` saves remaining dirty chunks before exit.
        app.insert_resource(self.config.clone())
            .insert_resource(ChunkStoreResource(store.clone()))
            .insert_resource(ChunkLoaderResource(Box::new(StoreLoader::new(store))))
            .insert_resource(SaveTicker::default())
            .add_systems(PostUpdate, save_dirty_chunks)
            .add_systems(Last, flush_on_exit);
    }
}
