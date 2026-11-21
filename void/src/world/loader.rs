//! Pluggable chunk-loading hook.
//!
//! By default the server generates every chunk from the active [`WorldGen`]. An
//! external persistence layer can install a [`ChunkLoaderResource`] to supply
//! chunks from disk *before* the generator runs. This mirrors the `Box<dyn>`
//! resource pattern used by [`WorldGen`] and keeps `void` free of any direct
//! dependency on a persistence crate.

use std::path::PathBuf;

use bevy_ecs::prelude::{Event, Resource};
use tracing::instrument;

use super::chunk_entity::ChunkData;
use super::chunk_pos::ChunkPos;
use super::dimension::DimensionId;
use super::generation::WorldGen;

/// A disk-backed (or otherwise external) source of chunk data, consulted before
/// the world generator.
pub trait ChunkLoader: Send + Sync {
    /// Returns the persisted chunk at `(dimension, pos)`, or `None` when no
    /// saved chunk exists (in which case the generator is used).
    ///
    /// A loader that finds a stored chunk it cannot read also returns `None`,
    /// so that the chunk is generated, and reports the failure with a
    /// [`ChunkLoadFailedEvent`].
    fn load_chunk(&self, dimension: DimensionId, pos: ChunkPos) -> Option<ChunkData>;
}

/// Why a stored chunk could not be loaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChunkLoadErrorKind {
    /// The stored bytes were read but could not be decoded (corrupt data).
    Decode,
    /// The stored bytes could not be read (an I/O error).
    Io,
}

impl ChunkLoadErrorKind {
    /// The value of the `kind` label of `voidmc_chunk_load_errors_total`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Decode => "decode",
            Self::Io => "io",
        }
    }
}

/// What the loader did to keep the stored bytes of a chunk it could not load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DamagedChunkAction {
    /// The stored bytes were copied, as found, to this side file, which the
    /// server never overwrites nor deletes. Later saves of the chunk proceed.
    Quarantined(PathBuf),
    /// The stored bytes could not be copied, so they are left where they are:
    /// saves of the chunk are skipped until the server stops.
    SavesSkipped,
}

/// Triggered when a [`ChunkLoader`] found a stored chunk it could not load.
/// The chunk is generated instead; `action` says what became of the stored
/// bytes. Each failure is also counted by `voidmc_chunk_load_errors_total`
/// on the metrics endpoint.
#[derive(Event, Clone, Debug)]
pub struct ChunkLoadFailedEvent {
    pub dimension: DimensionId,
    pub pos: ChunkPos,
    pub kind: ChunkLoadErrorKind,
    /// The error, as the loader logged it.
    pub error: String,
    pub action: DamagedChunkAction,
}

/// Resource wrapping the active [`ChunkLoader`]. Optional: when absent, chunks
/// are always generated (the default behavior).
#[derive(Resource)]
pub struct ChunkLoaderResource(pub Box<dyn ChunkLoader>);

/// Returns the chunk at `(dimension, pos)`, preferring the loader (if any) and
/// falling back to the world generator.
#[instrument(level = "info", skip(loader, world_gen))]
pub fn load_or_generate(
    loader: Option<&ChunkLoaderResource>,
    world_gen: &WorldGen,
    dimension: DimensionId,
    pos: &ChunkPos,
) -> ChunkData {
    if let Some(loader) = loader {
        if let Some(data) = loader.0.load_chunk(dimension, *pos) {
            return data;
        }
    }
    let generated_chunk = {
        let _span = tracing::info_span!("chunk_generation").entered();
        world_gen.0.generate_chunk(pos)
    };
    ChunkData::from_protocol_chunk(&generated_chunk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::generation::{DefaultWorldGenerator, WorldGenerator};
    use voidmc_protocol::clientbound::chunk::{ChunkHeightmaps, ChunkSection, LightData};

    fn marker_chunk(block_state_id: i32) -> ChunkData {
        let mut sections: Vec<ChunkSection> = (0..24).map(|_| ChunkSection::empty()).collect();
        sections[0].set_block_state(0, 0, 0, block_state_id);
        ChunkData::new(sections, ChunkHeightmaps::empty(), LightData::empty())
    }

    fn assert_generated(data: &ChunkData) {
        let surface = DefaultWorldGenerator::default().surface_height_at(0, 0);
        assert_eq!(
            data.get_block(0, surface - 1, 0),
            Some(voidmc_protocol::clientbound::chunk::blocks::GRASS_BLOCK)
        );
        assert_eq!(data.get_block(0, surface, 0), Some(0));
    }

    struct StubLoader(i32);
    impl ChunkLoader for StubLoader {
        fn load_chunk(&self, _dim: DimensionId, _pos: ChunkPos) -> Option<ChunkData> {
            Some(marker_chunk(self.0))
        }
    }

    struct MissingLoader;
    impl ChunkLoader for MissingLoader {
        fn load_chunk(&self, _dim: DimensionId, _pos: ChunkPos) -> Option<ChunkData> {
            None
        }
    }

    #[test]
    fn falls_back_to_generator_without_loader() {
        let world_gen = WorldGen(Box::new(DefaultWorldGenerator::default()));
        let data = load_or_generate(
            None,
            &world_gen,
            DimensionId::Overworld,
            &ChunkPos::new(0, 0),
        );
        assert_generated(&data);
    }

    #[test]
    fn falls_back_to_generator_when_loader_returns_none() {
        let world_gen = WorldGen(Box::new(DefaultWorldGenerator::default()));
        let loader = ChunkLoaderResource(Box::new(MissingLoader));
        let data = load_or_generate(
            Some(&loader),
            &world_gen,
            DimensionId::Overworld,
            &ChunkPos::new(0, 0),
        );
        assert_generated(&data);
    }

    #[test]
    fn prefers_loaded_chunk_over_generator() {
        let world_gen = WorldGen(Box::new(DefaultWorldGenerator::default()));
        let loader = ChunkLoaderResource(Box::new(StubLoader(42)));
        let data = load_or_generate(
            Some(&loader),
            &world_gen,
            DimensionId::Overworld,
            &ChunkPos::new(0, 0),
        );
        // The loaded chunk wins: its marker block survives instead of generated terrain.
        assert_eq!(data.get_block(0, super::super::CHUNK_MIN_Y, 0), Some(42));
    }
}
