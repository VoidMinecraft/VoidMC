//! Chunk -> ready viewers index backing the visibility tracker.
//!
//! Instead of recomputing an entity's viewers by scanning every ready player,
//! [`ChunkViewerIndex`] answers "who sees this `(dimension, chunk)`" directly.
//! It is kept in sync incrementally: [`maintain_chunk_viewer_index`] reacts to
//! the same change signals the tracker used to poll (`LoadedChunks`,
//! `PlayerDimension`, readiness), and [`on_player_leave`] purges a player the
//! moment it stops being ready or despawns. Every mutation records the touched
//! keys in [`DirtyChunks`] so the tracker only re-evaluates entities whose
//! viewer set could actually have changed.

use std::collections::HashSet;
use std::collections::hash_map::{Entry, HashMap};

use bevy_app::{App, PostUpdate};
use bevy_ecs::lifecycle::Remove;
use bevy_ecs::prelude::*;

use crate::components::{LoadedChunks, PlayerDimension, PlayerReady};
use crate::schedule::VoidSystems;
use crate::world::{ChunkPos, DimensionId};

pub(crate) type ChunkKey = (DimensionId, ChunkPos);

/// For each `(dimension, chunk)`, the set of ready players that have that chunk
/// loaded in that dimension. Membership is exactly "ready AND sees the chunk",
/// matching the rule the tracker enforces.
#[derive(Resource, Default)]
pub struct ChunkViewerIndex {
    map: HashMap<ChunkKey, HashSet<Entity>>,
}

impl ChunkViewerIndex {
    pub(crate) fn viewers(&self, key: ChunkKey) -> Option<&HashSet<Entity>> {
        self.map.get(&key)
    }

    fn insert(&mut self, key: ChunkKey, player: Entity) {
        self.map.entry(key).or_default().insert(player);
    }

    fn remove(&mut self, key: ChunkKey, player: Entity) {
        if let Entry::Occupied(mut entry) = self.map.entry(key) {
            entry.get_mut().remove(&player);
            if entry.get().is_empty() {
                entry.remove();
            }
        }
    }
}

/// Keys a player currently contributes to [`ChunkViewerIndex`]: the mirror we
/// diff against when its chunks or dimension change, and unwind on leave.
#[derive(Component, Default)]
pub(crate) struct IndexedChunks {
    dimension: Option<DimensionId>,
    chunks: HashSet<ChunkPos>,
}

/// Keys whose viewer set changed this tick. Populated by index upkeep, drained
/// by the tracker after it re-evaluates the affected entities.
#[derive(Resource, Default)]
pub struct DirtyChunks {
    keys: HashSet<ChunkKey>,
}

impl DirtyChunks {
    pub(crate) fn contains(&self, key: &ChunkKey) -> bool {
        self.keys.contains(key)
    }

    pub(crate) fn clear(&mut self) {
        self.keys.clear();
    }

    pub(crate) fn mark(&mut self, key: ChunkKey) {
        self.keys.insert(key);
    }
}

pub(super) fn register(app: &mut App) {
    app.init_resource::<ChunkViewerIndex>()
        .init_resource::<DirtyChunks>()
        .add_observer(on_player_leave)
        .add_systems(
            PostUpdate,
            maintain_chunk_viewer_index
                .after(VoidSystems::ChunkStreaming)
                .before(VoidSystems::EntityVisibility),
        );
}

/// Bring `mirror` and the index in line with a player's current dimension and
/// loaded chunks, marking every key whose membership moves.
fn reconcile(
    index: &mut ChunkViewerIndex,
    dirty: &mut DirtyChunks,
    player: Entity,
    dimension: DimensionId,
    current: &HashSet<ChunkPos>,
    mirror: &mut IndexedChunks,
) {
    if mirror.dimension == Some(dimension) {
        for chunk in mirror.chunks.difference(current) {
            index.remove((dimension, *chunk), player);
            dirty.mark((dimension, *chunk));
        }
        for chunk in current.difference(&mirror.chunks) {
            index.insert((dimension, *chunk), player);
            dirty.mark((dimension, *chunk));
        }
    } else {
        if let Some(old) = mirror.dimension {
            for chunk in &mirror.chunks {
                index.remove((old, *chunk), player);
                dirty.mark((old, *chunk));
            }
        }
        for chunk in current {
            index.insert((dimension, *chunk), player);
            dirty.mark((dimension, *chunk));
        }
    }

    mirror.dimension = Some(dimension);
    if mirror.chunks != *current {
        mirror.chunks.clone_from(current);
    }
}

/// Reacts to the exact change signals the tracker used to poll, so the index
/// reflects the same loaded-chunk state the tracker would have scanned.
fn maintain_chunk_viewer_index(
    mut index: ResMut<ChunkViewerIndex>,
    mut dirty: ResMut<DirtyChunks>,
    mut players: Query<
        (
            Entity,
            &PlayerDimension,
            Option<&LoadedChunks>,
            Option<&mut IndexedChunks>,
        ),
        (
            With<PlayerReady>,
            Or<(
                Changed<LoadedChunks>,
                Changed<PlayerDimension>,
                Added<PlayerReady>,
            )>,
        ),
    >,
    mut commands: Commands,
) {
    let empty = HashSet::new();
    for (player, dimension, loaded, mirror) in players.iter_mut() {
        let current = loaded.map(|l| &l.0).unwrap_or(&empty);
        match mirror {
            Some(mut mirror) => reconcile(
                &mut index,
                &mut dirty,
                player,
                dimension.0,
                current,
                &mut mirror,
            ),
            None => {
                let mut fresh = IndexedChunks::default();
                reconcile(
                    &mut index,
                    &mut dirty,
                    player,
                    dimension.0,
                    current,
                    &mut fresh,
                );
                commands.entity(player).insert(fresh);
            }
        }
    }
}

/// Drops a player from the index when it stops being ready or despawns. Runs
/// while the components still exist, so the mirror tells us exactly which keys
/// to unwind; it is reset in place so a later re-ready starts from empty.
fn on_player_leave(
    event: On<Remove, PlayerReady>,
    mut index: ResMut<ChunkViewerIndex>,
    mut dirty: ResMut<DirtyChunks>,
    mut mirrors: Query<&mut IndexedChunks>,
) {
    let Ok(mut mirror) = mirrors.get_mut(event.entity) else {
        return;
    };
    if let Some(dimension) = mirror.dimension {
        for chunk in &mirror.chunks {
            index.remove((dimension, *chunk), event.entity);
            dirty.mark((dimension, *chunk));
        }
    }
    mirror.dimension = None;
    mirror.chunks.clear();
}
