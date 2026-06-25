//! End-to-end persistence tests: save, "restart" (drop + reopen), reload.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use bevy_ecs::schedule::Schedule;
use voidmc::{
    ChunkData, ChunkDimension, ChunkDirty, ChunkLoader, ChunkPos, ChunkPosition, DimensionId,
};
use voidmc_protocol::clientbound::chunk::{ChunkHeightmaps, ChunkSection, LightData};
use voidmc_world_io::store::save_chunk;
use voidmc_world_io::systems::{SaveTicker, save_dirty_chunks};
use voidmc_world_io::{
    ChunkStore, ChunkStoreResource, PersistenceConfig, RegionChunkStore, StoreLoader,
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
