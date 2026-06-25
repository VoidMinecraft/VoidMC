//! Storage abstraction: the [`ChunkStore`] trait plus the ECS-facing resource
//! and the [`voidmc::ChunkLoader`] adapter.

use bevy_ecs::prelude::Resource;
use voidmc::{ChunkData, ChunkLoader, ChunkPos, DimensionId};

use crate::config::PersistenceConfig;
use crate::error::Result;
use crate::format;

/// A backend that persists raw (uncompressed) chunk NBT payloads keyed by
/// dimension and chunk position. Compression and on-disk layout are the
/// implementation's concern.
pub trait ChunkStore: Send + Sync {
    /// Returns the stored payload, or `None` when no chunk is saved.
    fn read(&self, dimension: DimensionId, pos: ChunkPos) -> Result<Option<Vec<u8>>>;
    /// Persists `payload` for `(dimension, pos)`.
    fn write(&self, dimension: DimensionId, pos: ChunkPos, payload: &[u8]) -> Result<()>;
    /// Flushes any buffered data to durable storage.
    fn flush(&self) -> Result<()>;
}

/// ECS resource wrapping the active [`ChunkStore`], used by the save system.
#[derive(Resource, Clone)]
pub struct ChunkStoreResource(pub std::sync::Arc<dyn ChunkStore>);

/// Adapts a [`ChunkStore`] into a [`voidmc::ChunkLoader`] so the engine can load
/// chunks from disk before generating them.
pub struct StoreLoader {
    store: std::sync::Arc<dyn ChunkStore>,
}

impl StoreLoader {
    pub fn new(store: std::sync::Arc<dyn ChunkStore>) -> Self {
        Self { store }
    }
}

impl ChunkLoader for StoreLoader {
    fn load_chunk(&self, dimension: DimensionId, pos: ChunkPos) -> Option<ChunkData> {
        match self.store.read(dimension, pos) {
            Ok(Some(bytes)) => match format::deserialize_chunk(&bytes) {
                Ok(loaded) => Some(loaded.data),
                Err(err) => {
                    tracing::error!(?err, x = pos.x, z = pos.z, "failed to decode saved chunk");
                    None
                }
            },
            Ok(None) => None,
            Err(err) => {
                tracing::error!(?err, x = pos.x, z = pos.z, "failed to read saved chunk");
                None
            }
        }
    }
}

/// Serializes a chunk and writes it through the store, honoring the light policy.
pub fn save_chunk(
    store: &dyn ChunkStore,
    config: &PersistenceConfig,
    dimension: DimensionId,
    pos: ChunkPos,
    data: &ChunkData,
) -> Result<()> {
    let bytes = format::serialize_chunk(
        dimension,
        pos.x,
        pos.z,
        data,
        !config.regenerate_light_on_load,
    )?;
    store.write(dimension, pos, &bytes)
}
