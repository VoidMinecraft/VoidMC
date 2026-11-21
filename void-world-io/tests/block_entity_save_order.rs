//! A block-entity change made in the tick of a periodic save reaches the disk.
//!
//! `sync_block_entities` marks a chunk dirty when one of its block entities
//! changed, and the periodic save clears the marker of every chunk it writes.
//! Both run in `PostUpdate` and act through deferred commands. Here a sign is
//! changed during `PostUpdate` of a save tick, after the save has written the
//! chunk and before the sync has marked it. If the save's clearing is applied
//! after the sync's mark, the chunk is left clean with the change unsaved:
//! not even the save on a graceful stop writes it.
//!
//! The executor applies deferred commands in the schedule's order, and the
//! schedule's order of the two systems depends on every other system of the
//! app, so each scenario runs with 0 to `MAX_EXTRA_SYSTEMS` unrelated systems
//! added. Under the multi-threaded executor, a slow system that writes what
//! the sync reads (and the save does not) lets the save run first.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use bevy_app::{App, AppExit, First, PostUpdate, Update};
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::IntoScheduleConfigs;
use voidmc::components::LoadedChunks;
use voidmc::{
    BlockEntities, BlockEntity, BlockPosition, ChunkData, ChunkDirty, ChunkIndex, ChunkLoader,
    ChunkPos, DimensionId, ExecutionMode, ServerConfigBuilder, ServerConfigResource, Sign,
    VoidServer,
};
use voidmc_data::v26_1_2::blocks::OAK_SIGN;
use voidmc_world_io::{
    ChunkStore, PersistenceConfig, RegionChunkStore, StoreLoader, WorldPersistencePlugin,
};

const SAVE_INTERVAL: u64 = 4;
/// Places the sign, saved by the first periodic save.
const PLACE_TICK: u64 = 1;
/// Changes the sign, which marks the chunk dirty until the next save.
const FIRST_CHANGE_TICK: u64 = 2 * SAVE_INTERVAL - 2;
/// A periodic save writes the chunk, then the sign changes again.
const SAVE_TICK: u64 = 2 * SAVE_INTERVAL;
const PLACED_TEXT: &str = "placed";
const FIRST_CHANGE_TEXT: &str = "first change";
const SAVE_TICK_TEXT: &str = "changed in the save tick";
const TEXTS: [&str; 3] = [PLACED_TEXT, FIRST_CHANGE_TEXT, SAVE_TICK_TEXT];
const STOP_TICK: u64 = SAVE_TICK + 2;
const MAX_EXTRA_SYSTEMS: usize = 24;
const SLOW_SYSTEM: Duration = Duration::from_millis(100);

/// The sign, in the spawn chunk, high above generated terrain.
const LOCAL_X: u8 = 3;
const WORLD_Y: i32 = 300;
const LOCAL_Z: u8 = 11;

#[derive(Resource, Default)]
struct Tick(u64);

fn count_ticks(mut tick: ResMut<Tick>) {
    tick.0 += 1;
}

fn spawn_chunk(config: &ServerConfigResource) -> ChunkPos {
    ChunkPos::from_block(config.spawn_x, config.spawn_z)
}

fn sign_position(chunk: ChunkPos) -> BlockPosition {
    BlockPosition {
        x: chunk.x * 16 + i32::from(LOCAL_X),
        y: WORLD_Y as i16,
        z: chunk.z * 16 + i32::from(LOCAL_Z),
    }
}

fn sign(text: &str) -> BlockEntity {
    Sign::lines([text, "", "", ""]).into()
}

fn place_sign_block(
    tick: Res<Tick>,
    index: Res<ChunkIndex>,
    config: Res<ServerConfigResource>,
    mut chunks: Query<&mut ChunkData>,
    mut commands: Commands,
) {
    if tick.0 != PLACE_TICK {
        return;
    }
    let entity = index.0[&(DimensionId::Overworld, spawn_chunk(&config))];
    chunks
        .get_mut(entity)
        .expect("the spawn chunk has data")
        .set_block(LOCAL_X, WORLD_Y, LOCAL_Z, OAK_SIGN)
        .expect("y in range");
    commands.entity(entity).insert(ChunkDirty);
}

/// Writes the sign in `Update` when it is placed and when it first changes.
fn write_sign_in_update(
    tick: Res<Tick>,
    config: Res<ServerConfigResource>,
    mut block_entities: BlockEntities,
) {
    let text = match tick.0 {
        PLACE_TICK => PLACED_TEXT,
        FIRST_CHANGE_TICK => FIRST_CHANGE_TEXT,
        _ => return,
    };
    block_entities
        .set(
            DimensionId::Overworld,
            sign_position(spawn_chunk(&config)),
            sign(text),
        )
        .expect("the sign block hosts a sign");
}

/// Changes the sign during `PostUpdate` of the save tick.
fn write_sign_in_post_update(
    tick: Res<Tick>,
    config: Res<ServerConfigResource>,
    mut block_entities: BlockEntities,
) {
    if tick.0 != SAVE_TICK {
        return;
    }
    block_entities
        .set(
            DimensionId::Overworld,
            sign_position(spawn_chunk(&config)),
            sign(SAVE_TICK_TEXT),
        )
        .expect("the sign block hosts a sign");
}

/// A slow `PostUpdate` system of the save tick that writes the players'
/// loaded chunks, which the block-entity sync reads.
fn slow_system(tick: Res<Tick>, _loaded: Query<&mut LoadedChunks>) {
    if tick.0 == SAVE_TICK {
        std::thread::sleep(SLOW_SYSTEM);
    }
}

fn stop(tick: Res<Tick>, mut exit: MessageWriter<AppExit>) {
    if tick.0 == STOP_TICK {
        exit.write(AppExit::Success);
    }
}

fn unrelated_system() {}

/// Runs a server over `world` until `STOP_TICK`, then stops it gracefully.
fn run_server(world: &Path, mode: ExecutionMode, extra_systems: usize) {
    let world = world.to_path_buf();
    let config = ServerConfigBuilder::new()
        .address("127.0.0.1:0")
        .spawn_chunk_radius(1)
        .tick_rate(200)
        .execution_mode(mode)
        .build();
    let exit = VoidServer::new(config)
        .add_plugin(move |app: &mut App| {
            app.init_resource::<Tick>()
                .add_systems(First, count_ticks)
                .add_systems(
                    Update,
                    ((place_sign_block, write_sign_in_update).chain(), stop),
                )
                .add_systems(PostUpdate, slow_system)
                .add_plugins(WorldPersistencePlugin::new(
                    PersistenceConfig::new(world).save_interval_ticks(SAVE_INTERVAL),
                ))
                .add_systems(PostUpdate, write_sign_in_post_update);
            for _ in 0..extra_systems {
                app.add_systems(PostUpdate, unrelated_system);
            }
        })
        .run();
    assert_eq!(exit, AppExit::Success);
}

/// The sign stored on disk.
fn stored_sign(world: &Path) -> Option<BlockEntity> {
    let chunk = spawn_chunk(&ServerConfigResource::from(
        &ServerConfigBuilder::new().build(),
    ));
    let store: Arc<dyn ChunkStore> = Arc::new(RegionChunkStore::new(world));
    StoreLoader::new(store)
        .load_chunk(DimensionId::Overworld, chunk)
        .expect("the spawn chunk was saved")
        .block_entity(chunk, sign_position(chunk))
        .cloned()
}

/// The servers run one after another: one test, so that two servers never
/// share the process's task pools.
#[test]
fn sign_changed_in_a_save_tick_is_saved() {
    let mut lost = Vec::new();
    for mode in [ExecutionMode::SingleThreaded, ExecutionMode::MultiThreaded] {
        for extra_systems in 0..=MAX_EXTRA_SYSTEMS {
            let world = tempfile::tempdir().unwrap();
            run_server(world.path(), mode, extra_systems);
            let stored = stored_sign(world.path());
            if stored != Some(sign(SAVE_TICK_TEXT)) {
                let found = TEXTS
                    .into_iter()
                    .find(|text| stored == Some(sign(text)))
                    .map_or_else(|| format!("{stored:?}"), |text| format!("sign {text:?}"));
                lost.push(format!("{mode:?}, {extra_systems} extra systems: {found}"));
            }
        }
    }
    assert!(
        lost.is_empty(),
        "the sign changed in the save tick is not on disk after a graceful stop \
         (stored instead):\n{}",
        lost.join("\n")
    );
}
