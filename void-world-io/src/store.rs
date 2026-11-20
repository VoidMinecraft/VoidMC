//! Storage abstraction: the [`ChunkStore`] trait plus the ECS-facing resource
//! and the [`voidmc::ChunkLoader`] adapter.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use bevy_ecs::prelude::Resource;
use voidmc::{
    ChunkData, ChunkLoadErrorKind, ChunkLoadFailedEvent, ChunkLoader, ChunkPos, DamagedChunkAction,
    DimensionId,
};

use crate::config::PersistenceConfig;
use crate::error::{PersistenceError, Result};
use crate::format;

/// A backend that persists raw (uncompressed) chunk NBT payloads keyed by
/// dimension and chunk position. Compression and on-disk layout are the
/// implementation's concern.
pub trait ChunkStore: Send + Sync {
    /// Returns the stored payload, or `None` when no chunk is saved.
    ///
    /// Fails with [`PersistenceError::Io`] when the stored bytes cannot be
    /// read, and with [`PersistenceError::Corrupt`] when they were read but
    /// cannot be decoded.
    fn read(&self, dimension: DimensionId, pos: ChunkPos) -> Result<Option<Vec<u8>>>;
    /// Persists `payload` for `(dimension, pos)`.
    fn write(&self, dimension: DimensionId, pos: ChunkPos, payload: &[u8]) -> Result<()>;
    /// Flushes any buffered data to durable storage.
    fn flush(&self) -> Result<()>;
    /// Copies the bytes stored for `(dimension, pos)`, exactly as found, to a
    /// new file that the store never overwrites nor deletes, and returns its
    /// path; `None` when the store keeps no such copy.
    ///
    /// Called when the stored chunk cannot be decoded, before anything can
    /// overwrite it. Without a copy, the loader skips every save of that
    /// chunk until the server stops, so that the stored bytes stay as they
    /// are. The default implementation keeps no copy.
    fn quarantine(&self, dimension: DimensionId, pos: ChunkPos) -> Result<Option<PathBuf>> {
        let _ = (dimension, pos);
        Ok(None)
    }
}

/// ECS resource wrapping the active [`ChunkStore`], used by the save system.
#[derive(Resource, Clone)]
pub struct ChunkStoreResource(pub Arc<dyn ChunkStore>);

/// The stored chunks the loader could not load, shared by the
/// [`StoreLoader`] and the save systems.
#[derive(Default)]
pub(crate) struct LoadFailures {
    /// Failures not yet triggered as [`ChunkLoadFailedEvent`]s.
    pending: Mutex<Vec<ChunkLoadFailedEvent>>,
    /// Chunks whose stored bytes must not be overwritten in this session.
    skip_saves: Mutex<HashSet<(DimensionId, ChunkPos)>>,
}

impl LoadFailures {
    pub(crate) fn take_pending(&self) -> Vec<ChunkLoadFailedEvent> {
        std::mem::take(&mut *self.pending.lock().expect("load failures mutex poisoned"))
    }

    pub(crate) fn skips_saves_of(&self, dimension: DimensionId, pos: ChunkPos) -> bool {
        self.skip_saves
            .lock()
            .expect("load failures mutex poisoned")
            .contains(&(dimension, pos))
    }

    fn record(&self, failure: ChunkLoadFailedEvent) {
        if failure.action == DamagedChunkAction::SavesSkipped {
            self.skip_saves
                .lock()
                .expect("load failures mutex poisoned")
                .insert((failure.dimension, failure.pos));
        }
        self.pending
            .lock()
            .expect("load failures mutex poisoned")
            .push(failure);
    }
}

/// ECS resource holding the failures of the [`StoreLoader`] installed by
/// [`WorldPersistencePlugin`](crate::WorldPersistencePlugin): the save systems
/// read it to skip the chunks whose stored bytes must stay as they are.
#[derive(Resource, Clone)]
pub struct LoadFailuresResource(pub(crate) Arc<LoadFailures>);

/// Adapts a [`ChunkStore`] into a [`voidmc::ChunkLoader`] so the engine can load
/// chunks from disk before generating them.
///
/// A stored chunk that cannot be loaded is generated instead (the loader
/// returns `None`). Before that, an undecodable chunk is quarantined with
/// [`ChunkStore::quarantine`]; a chunk that cannot be read, or whose copy
/// fails, has its saves skipped until the server stops. Installed by
/// [`WorldPersistencePlugin`](crate::WorldPersistencePlugin), it also triggers
/// a [`ChunkLoadFailedEvent`] for each such chunk.
pub struct StoreLoader {
    store: Arc<dyn ChunkStore>,
    failures: Arc<LoadFailures>,
}

impl StoreLoader {
    pub fn new(store: Arc<dyn ChunkStore>) -> Self {
        Self::sharing(store, Arc::default())
    }

    pub(crate) fn sharing(store: Arc<dyn ChunkStore>, failures: Arc<LoadFailures>) -> Self {
        Self { store, failures }
    }

    /// Keeps the stored bytes of a chunk that could not be loaded and records
    /// the failure.
    fn keep_damaged(
        &self,
        dimension: DimensionId,
        pos: ChunkPos,
        kind: ChunkLoadErrorKind,
        error: PersistenceError,
    ) {
        let (x, z) = (pos.x, pos.z);
        let dim = dimension.name();
        let error = error.to_string();
        let action = match kind {
            ChunkLoadErrorKind::Io => {
                tracing::error!(
                    dim, x, z, %error,
                    "cannot read stored chunk; generating it, and skipping its saves until the server stops so that the stored bytes stay as they are"
                );
                DamagedChunkAction::SavesSkipped
            }
            ChunkLoadErrorKind::Decode => match self.store.quarantine(dimension, pos) {
                Ok(Some(path)) => {
                    tracing::error!(
                        dim, x, z, %error, quarantined = %path.display(),
                        "cannot decode stored chunk; its bytes are copied to the quarantine file and it is generated again"
                    );
                    DamagedChunkAction::Quarantined(path)
                }
                Ok(None) => {
                    tracing::error!(
                        dim, x, z, %error,
                        "cannot decode stored chunk and the store keeps no copy of it; generating it, and skipping its saves until the server stops so that the stored bytes stay as they are"
                    );
                    DamagedChunkAction::SavesSkipped
                }
                Err(copy_error) => {
                    tracing::error!(
                        dim, x, z, %error, %copy_error,
                        "cannot decode stored chunk nor copy it to a quarantine file; generating it, and skipping its saves until the server stops so that the stored bytes stay as they are"
                    );
                    DamagedChunkAction::SavesSkipped
                }
            },
        };
        self.failures.record(ChunkLoadFailedEvent {
            dimension,
            pos,
            kind,
            error,
            action,
        });
    }
}

impl ChunkLoader for StoreLoader {
    fn load_chunk(&self, dimension: DimensionId, pos: ChunkPos) -> Option<ChunkData> {
        let (kind, error) = match self.store.read(dimension, pos) {
            Ok(None) => return None,
            Ok(Some(bytes)) => match format::deserialize_chunk(&bytes) {
                Ok(loaded) => return Some(loaded.data),
                Err(err) => (ChunkLoadErrorKind::Decode, err),
            },
            Err(err @ PersistenceError::Io(_)) => (ChunkLoadErrorKind::Io, err),
            Err(err @ PersistenceError::Corrupt(_)) => (ChunkLoadErrorKind::Decode, err),
        };
        self.keep_damaged(dimension, pos, kind, error);
        None
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
