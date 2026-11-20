//! ECS systems that drive saving: periodic flush and a shutdown flush.

use bevy_app::AppExit;
use bevy_ecs::prelude::*;
use voidmc::{ChunkData, ChunkDimension, ChunkDirty, ChunkPosition};

use crate::config::PersistenceConfig;
use crate::store::{ChunkStoreResource, LoadFailuresResource, save_chunk};

/// Counts ticks between periodic saves.
#[derive(Resource, Default)]
pub struct SaveTicker(pub u64);

type DirtyQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static ChunkPosition,
        &'static ChunkDimension,
        &'static ChunkData,
    ),
    With<ChunkDirty>,
>;

/// Writes dirty chunks to the store and clears their `ChunkDirty` marker on
/// success. `limit` of `0` means unlimited. Returns the number saved.
///
/// A chunk whose stored bytes the loader could neither read nor copy is not
/// written: its marker is cleared and the stored bytes stay as they are.
fn flush_dirty(
    config: &PersistenceConfig,
    store: &ChunkStoreResource,
    failures: Option<&LoadFailuresResource>,
    dirty: &DirtyQuery,
    commands: &mut Commands,
    limit: usize,
) -> usize {
    let mut saved = 0usize;
    for (entity, pos, dim, data) in dirty.iter() {
        if limit != 0 && saved >= limit {
            break;
        }
        if failures.is_some_and(|failures| failures.0.skips_saves_of(dim.0, pos.0)) {
            tracing::warn!(
                dim = dim.0.name(),
                x = pos.0.x,
                z = pos.0.z,
                "not saving chunk: its stored bytes could not be read nor copied when it was loaded, and they are kept until the server stops"
            );
            commands.entity(entity).remove::<ChunkDirty>();
            continue;
        }
        match save_chunk(store.0.as_ref(), config, dim.0, pos.0, data) {
            Ok(()) => {
                commands.entity(entity).remove::<ChunkDirty>();
                saved += 1;
            }
            Err(err) => {
                tracing::error!(?err, x = pos.0.x, z = pos.0.z, "chunk save failed");
            }
        }
    }
    saved
}

/// Periodically flushes dirty chunks, gated by `save_interval_ticks`.
pub fn save_dirty_chunks(
    mut ticker: ResMut<SaveTicker>,
    config: Res<PersistenceConfig>,
    store: Res<ChunkStoreResource>,
    failures: Option<Res<LoadFailuresResource>>,
    dirty: DirtyQuery,
    mut commands: Commands,
) {
    ticker.0 += 1;
    if ticker.0 < config.save_interval_ticks {
        return;
    }
    ticker.0 = 0;
    let saved = flush_dirty(
        &config,
        &store,
        failures.as_deref(),
        &dirty,
        &mut commands,
        config.max_saves_per_flush,
    );
    if saved > 0 {
        tracing::info!(saved, "World saved");
    }
}

/// On any `AppExit` (from `/stop`, Ctrl-C, or anything else), force-saves every
/// remaining dirty chunk and flushes the store before the server exits.
///
/// Runs in the `Last` schedule, so it observes an `AppExit` written earlier in
/// the same tick and completes before the runner stops the loop.
pub fn flush_on_exit(
    mut exit: MessageReader<AppExit>,
    config: Res<PersistenceConfig>,
    store: Res<ChunkStoreResource>,
    failures: Option<Res<LoadFailuresResource>>,
    dirty: DirtyQuery,
    mut commands: Commands,
) {
    if exit.is_empty() {
        return;
    }
    exit.clear();
    let saved = flush_dirty(
        &config,
        &store,
        failures.as_deref(),
        &dirty,
        &mut commands,
        0,
    );
    if let Err(err) = store.0.flush() {
        tracing::error!(?err, "world persistence: shutdown flush failed");
    }
    tracing::info!(saved, "world persistence: flushed on shutdown");
}

/// Triggers a [`voidmc::ChunkLoadFailedEvent`] for each stored chunk the
/// loader could not load since the last run.
pub(crate) fn report_load_failures(failures: Res<LoadFailuresResource>, mut commands: Commands) {
    for failure in failures.0.take_pending() {
        commands.trigger(failure);
    }
}
