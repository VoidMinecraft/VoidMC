//! End-to-end persistence tests: save, "restart" (drop + reopen), reload.

use std::sync::{Arc, Mutex};

use bevy_app::{App, AppExit};
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::Schedule;
use voidmc::{
    ChunkData, ChunkDimension, ChunkDirty, ChunkLoader, ChunkLoaderResource, ChunkPos,
    ChunkPosition, DimensionId,
};
use voidmc_protocol::clientbound::chunk::{ChunkHeightmaps, ChunkSection, LightData};
use voidmc_world_io::store::save_chunk;
use voidmc_world_io::systems::{SaveTicker, save_dirty_chunks};
use voidmc_world_io::{
    ChunkStore, ChunkStoreResource, PersistenceConfig, PersistenceError, RegionChunkStore, Result,
    StoreLoader, WorldPersistencePlugin,
};

fn chunk_with_block(world_y: i32, id: i32) -> ChunkData {
    let sections: Vec<ChunkSection> = (0..24).map(|_| ChunkSection::empty()).collect();
    let mut data = ChunkData::new(
        sections,
        ChunkHeightmaps::empty(),
        LightData::full_sky_light(),
    );
    data.set_block(3, world_y, 11, id).expect("y in range");
    data
}

#[test]
fn save_then_restart_loads_modified_block() {
    let dir = tempfile::tempdir().unwrap();
    let config = PersistenceConfig::new(dir.path());
    let pos = ChunkPos::new(2, -3);
    // Use a distinct id so a regen-vs-load mismatch would be obvious.
    let data = chunk_with_block(70, 1234);

    {
        let store = RegionChunkStore::new(dir.path());
        save_chunk(&store, &config, DimensionId::Overworld, pos, &data).unwrap();
        store.flush().unwrap();
    }

    // "Restart": a fresh store + loader over the same directory.
    let store: Arc<dyn ChunkStore> = Arc::new(RegionChunkStore::new(dir.path()));
    let loader = StoreLoader::new(store);
    let loaded = loader
        .load_chunk(DimensionId::Overworld, pos)
        .expect("chunk should load from disk");
    assert_eq!(loaded.get_block(3, 70, 11), Some(1234));
}

#[test]
fn save_system_consumes_dirty_and_persists() {
    let dir = tempfile::tempdir().unwrap();
    let mut world = World::new();
    world.insert_resource(PersistenceConfig::new(dir.path()).save_interval_ticks(1));
    let store = Arc::new(RegionChunkStore::new(dir.path()));
    world.insert_resource(ChunkStoreResource(store));
    world.insert_resource(SaveTicker::default());

    let data = chunk_with_block(64, 777);
    let entity = world
        .spawn((
            ChunkPosition(ChunkPos::new(0, 0)),
            ChunkDimension(DimensionId::Overworld),
            data,
            ChunkDirty,
        ))
        .id();

    let mut schedule = Schedule::default();
    schedule.add_systems(save_dirty_chunks);
    schedule.run(&mut world);

    // The dirty marker is cleared once the chunk is saved...
    assert!(
        world.get::<ChunkDirty>(entity).is_none(),
        "ChunkDirty should be removed after a successful save"
    );
    // ...and the region file exists on disk.
    assert!(dir.path().join("region/overworld/r.0.0.vrm").exists());
}

#[test]
fn unsaved_chunk_is_a_load_miss() {
    let dir = tempfile::tempdir().unwrap();
    let store: Arc<dyn ChunkStore> = Arc::new(RegionChunkStore::new(dir.path()));
    let loader = StoreLoader::new(store);
    assert!(
        loader
            .load_chunk(DimensionId::Overworld, ChunkPos::new(5, 5))
            .is_none()
    );
}

/// A region store whose reads of one chunk fail with an I/O error, as a
/// failing disk would. Records the chunks written through it.
struct UnreadableChunk {
    inner: RegionChunkStore,
    unreadable: ChunkPos,
    written: Mutex<Vec<ChunkPos>>,
}

impl ChunkStore for UnreadableChunk {
    fn read(&self, dimension: DimensionId, pos: ChunkPos) -> Result<Option<Vec<u8>>> {
        if pos == self.unreadable {
            return Err(PersistenceError::Io(std::io::Error::other(
                "injected read failure",
            )));
        }
        self.inner.read(dimension, pos)
    }

    fn write(&self, dimension: DimensionId, pos: ChunkPos, payload: &[u8]) -> Result<()> {
        self.written.lock().unwrap().push(pos);
        self.inner.write(dimension, pos, payload)
    }

    fn flush(&self) -> Result<()> {
        self.inner.flush()
    }
}

#[test]
fn an_unreadable_chunk_is_generated_but_not_saved_over_for_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let config = PersistenceConfig::new(dir.path()).save_interval_ticks(1);
    let (unreadable, readable) = (ChunkPos::new(0, 0), ChunkPos::new(1, 0));
    {
        let store = RegionChunkStore::new(dir.path());
        let saved = chunk_with_block(70, 1234);
        save_chunk(&store, &config, DimensionId::Overworld, unreadable, &saved).unwrap();
        store.flush().unwrap();
    }

    let store = Arc::new(UnreadableChunk {
        inner: RegionChunkStore::new(dir.path()),
        unreadable,
        written: Mutex::new(Vec::new()),
    });
    let mut app = App::new();
    app.add_plugins(WorldPersistencePlugin::with_store(
        config.clone(),
        store.clone(),
    ));
    let loaded = app
        .world()
        .resource::<ChunkLoaderResource>()
        .0
        .load_chunk(DimensionId::Overworld, unreadable);
    assert!(loaded.is_none(), "an unreadable chunk is generated");

    // Gameplay edits the generated chunk and another one; a periodic save
    // and the shutdown flush follow.
    for pos in [unreadable, readable] {
        app.world_mut().spawn((
            ChunkPosition(pos),
            ChunkDimension(DimensionId::Overworld),
            chunk_with_block(64, 777),
            ChunkDirty,
        ));
    }
    app.update();
    app.world_mut().write_message(AppExit::Success);
    app.update();

    assert_eq!(*store.written.lock().unwrap(), [readable]);
    drop(app);
    drop(store);
    let reopened: Arc<dyn ChunkStore> = Arc::new(RegionChunkStore::new(dir.path()));
    let kept = StoreLoader::new(reopened)
        .load_chunk(DimensionId::Overworld, unreadable)
        .expect("the stored chunk is still on disk");
    assert_eq!(kept.get_block(3, 70, 11), Some(1234));
}
