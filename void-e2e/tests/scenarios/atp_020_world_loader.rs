//! VMC-ATP-020: chunks come from the world loader hook before the generator.
//!
//! A client joins a server with a `ChunkLoader` installed and compares every
//! block of the chunks it receives with the loader's chunk (for stored
//! positions) or with the generator's chunk (for missing positions). The
//! second scenario does the same through `voidmc-world-io`'s
//! `WorldPersistencePlugin` and its `RegionChunkStore`, then restarts the
//! server over the same directory to check that an edit survives.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use bevy_app::PostStartup;
use bevy_ecs::system::ResMut;
use voidmc::{
    ChunkData, ChunkLoader, ChunkLoaderResource, ChunkPos as VoidChunkPos, DefaultWorldGenerator,
    DimensionId, ServerConfigResource, WorldGenerator,
};
use voidmc_e2e::block::BlockState;
use voidmc_e2e::core::direction::Direction;
use voidmc_e2e::core::position::{BlockPos, ChunkPos};
use voidmc_e2e::protocol::packets::game::{
    ClientboundGamePacket,
    s_player_action::{Action, ServerboundPlayerAction},
};
use voidmc_e2e::registry::builtin::BlockKind;
use voidmc_e2e::{Bot, TestServer, TestServerBuilder};
use voidmc_protocol::clientbound::Chunk;
use voidmc_protocol::clientbound::chunk::{ChunkHeightmaps, ChunkSection, LightData};
use voidmc_world_io::store::save_chunk;
use voidmc_world_io::{ChunkStore, PersistenceConfig, RegionChunkStore, WorldPersistencePlugin};

use crate::{ground, wait_until_ready};

const MIN_Y: i32 = -64;
const MAX_Y: i32 = 320;

/// Stored chunk inside the spawn area, loaded while the world starts.
const STORED_AT_SPAWN: (i32, i32) = (1, 0);
/// Stored chunk outside the spawn area, loaded when it is streamed to a player.
const STORED_STREAMED: (i32, i32) = (-3, 2);
/// Missing chunk inside the spawn area.
const MISSING_AT_SPAWN: (i32, i32) = (-1, 0);
/// Missing chunk outside the spawn area.
const MISSING_STREAMED: (i32, i32) = (3, -2);
/// Stored chunk far from the spawn area, where a plugin moves the spawn.
const STORED_AWAY: (i32, i32) = (9, -7);

/// The test servers generate a 3×3 spawn area around chunk (0, 0); a view
/// distance of 4 streams the rest.
fn builder() -> TestServerBuilder {
    TestServer::builder().config(|config| config.spawn_chunk_radius(1).view_distance(4))
}

fn void_pos((x, z): (i32, i32)) -> VoidChunkPos {
    VoidChunkPos::new(x, z)
}

fn client_pos((x, z): (i32, i32)) -> ChunkPos {
    ChunkPos::new(x, z)
}

fn state_id(kind: BlockKind) -> i32 {
    u32::from(BlockState::from(kind)) as i32
}

/// A chunk no generator produces: all air but a few marker blocks, one of
/// them in the lowest section where generated terrain is solid stone.
fn stored_chunk() -> ChunkData {
    let sections: Vec<ChunkSection> = (0..24).map(|_| ChunkSection::empty()).collect();
    let mut data = ChunkData::new(
        sections,
        ChunkHeightmaps::empty(),
        LightData::full_sky_light(),
    );
    for (x, y, z, kind) in [
        (3, 70, 11, BlockKind::DiamondBlock),
        (0, MIN_Y, 0, BlockKind::GoldBlock),
        (15, 200, 15, BlockKind::EmeraldBlock),
    ] {
        data.set_block(x, y, z, state_id(kind)).expect("y in range");
    }
    data
}

fn generated_chunk(pos: (i32, i32)) -> ChunkData {
    ChunkData::from_protocol_chunk(&DefaultWorldGenerator::default().generate_chunk(&void_pos(pos)))
}

/// Compares every block of the chunk the bot holds at `pos` with `expected`.
fn assert_client_chunk(bot: &Bot, pos: (i32, i32), expected: &ChunkData, what: &str) {
    let chunk = client_pos(pos);
    let mismatches: Vec<String> = bot.view(|view| {
        assert!(
            view.chunks.contains_key(&chunk),
            "{} does not hold chunk {pos:?} ({what})",
            bot.name()
        );
        let mut mismatches = Vec::new();
        for x in 0..16u8 {
            for z in 0..16u8 {
                for y in MIN_Y..MAX_Y {
                    let block =
                        BlockPos::new(pos.0 * 16 + i32::from(x), y, pos.1 * 16 + i32::from(z));
                    let seen = view.block_at(block).map(|state| u32::from(state) as i32);
                    let wanted = expected.get_block(x, y, z);
                    if seen != wanted {
                        mismatches.push(format!("{block:?}: client {seen:?}, expected {wanted:?}"));
                    }
                }
            }
        }
        mismatches
    });
    assert!(
        mismatches.is_empty(),
        "chunk {pos:?} is not {what}: {} block(s) differ, first: {:?}",
        mismatches.len(),
        &mismatches[..mismatches.len().min(5)]
    );
}

async fn wait_for_chunks(bot: &Bot, positions: &[(i32, i32)]) {
    let wanted: Vec<ChunkPos> = positions.iter().copied().map(client_pos).collect();
    bot.wait_until("the stored and missing chunks", |view| {
        wanted.iter().all(|pos| view.chunks.contains_key(pos))
    })
    .await;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Call {
    Load(VoidChunkPos),
    Generate(VoidChunkPos),
}

type CallLog = Arc<Mutex<Vec<Call>>>;

/// A test loader that holds a fixed set of chunks and records every lookup.
struct FixedLoader {
    stored: Vec<VoidChunkPos>,
    calls: CallLog,
}

impl ChunkLoader for FixedLoader {
    fn load_chunk(&self, dimension: DimensionId, pos: VoidChunkPos) -> Option<ChunkData> {
        assert_eq!(dimension, DimensionId::Overworld);
        self.calls.lock().unwrap().push(Call::Load(pos));
        self.stored.contains(&pos).then(stored_chunk)
    }
}

/// The default generator, recording every chunk it generates.
struct RecordingGenerator {
    inner: DefaultWorldGenerator,
    calls: CallLog,
}

impl WorldGenerator for RecordingGenerator {
    fn generate_chunk(&self, pos: &VoidChunkPos) -> Chunk {
        self.calls.lock().unwrap().push(Call::Generate(*pos));
        self.inner.generate_chunk(pos)
    }

    fn surface_height_at(&self, block_x: i32, block_z: i32) -> i32 {
        self.inner.surface_height_at(block_x, block_z)
    }
}

fn first(calls: &[Call], call: Call) -> Option<usize> {
    calls.iter().position(|recorded| *recorded == call)
}

#[tokio::test]
async fn loader_chunks_replace_generation_and_missing_chunks_are_generated() {
    let calls = CallLog::default();
    let loader_calls = calls.clone();
    let generator_calls = calls.clone();
    let server = builder()
        .config(move |config| {
            config.world_generator(RecordingGenerator {
                inner: DefaultWorldGenerator::default(),
                calls: generator_calls,
            })
        })
        .plugin(move |app| {
            app.insert_resource(ChunkLoaderResource(Box::new(FixedLoader {
                stored: vec![void_pos(STORED_AT_SPAWN), void_pos(STORED_STREAMED)],
                calls: loader_calls,
            })));
        })
        .start()
        .await;

    // The spawn area went through the loader while the world started; the
    // chunks outside it have not been asked for yet.
    let at_startup = calls.lock().unwrap().clone();
    for pos in [STORED_AT_SPAWN, MISSING_AT_SPAWN] {
        assert!(
            at_startup.contains(&Call::Load(void_pos(pos))),
            "the spawn-area chunk {pos:?} was not looked up at startup: {at_startup:?}"
        );
    }
    for pos in [STORED_STREAMED, MISSING_STREAMED] {
        assert!(
            !at_startup.contains(&Call::Load(void_pos(pos))),
            "the chunk {pos:?} outside the spawn area was looked up at startup"
        );
    }

    let alice = server.join("Alice").await;
    wait_for_chunks(
        &alice,
        &[
            STORED_AT_SPAWN,
            STORED_STREAMED,
            MISSING_AT_SPAWN,
            MISSING_STREAMED,
        ],
    )
    .await;

    let calls = calls.lock().unwrap().clone();
    let stored = stored_chunk();
    for pos in [STORED_AT_SPAWN, STORED_STREAMED] {
        assert_client_chunk(&alice, pos, &stored, "the loader's chunk");
        assert!(
            first(&calls, Call::Load(void_pos(pos))).is_some(),
            "the loader was not asked for {pos:?}"
        );
        assert_eq!(
            first(&calls, Call::Generate(void_pos(pos))),
            None,
            "the stored chunk {pos:?} was generated"
        );
    }
    for pos in [MISSING_AT_SPAWN, MISSING_STREAMED] {
        assert_client_chunk(&alice, pos, &generated_chunk(pos), "generated terrain");
        let load = first(&calls, Call::Load(void_pos(pos)))
            .unwrap_or_else(|| panic!("the loader was not asked for {pos:?}"));
        let generate = first(&calls, Call::Generate(void_pos(pos)))
            .unwrap_or_else(|| panic!("the missing chunk {pos:?} was not generated"));
        assert!(
            load < generate,
            "the missing chunk {pos:?} was generated before the loader was asked"
        );
    }

    alice.assert_healthy();
    server.stop().await;
}

/// A world directory under the system temp directory, removed on drop.
struct TempWorld(PathBuf);

impl TempWorld {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "voidmc-e2e-atp-020-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("failed to create the temporary world directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempWorld {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn persistent(world: &TempWorld) -> TestServerBuilder {
    let config = PersistenceConfig::new(world.path());
    builder().plugin(move |app| {
        app.add_plugins(WorldPersistencePlugin::new(config));
    })
}

#[tokio::test]
async fn persistence_plugin_loads_stored_chunks_and_keeps_edits_across_restarts() {
    let world = TempWorld::new();
    {
        let store = RegionChunkStore::new(world.path());
        let config = PersistenceConfig::new(world.path());
        for pos in [STORED_AT_SPAWN, STORED_STREAMED] {
            save_chunk(
                &store,
                &config,
                DimensionId::Overworld,
                void_pos(pos),
                &stored_chunk(),
            )
            .expect("failed to store the chunk");
        }
        store.flush().expect("failed to flush the store");
    }
    let all = [
        STORED_AT_SPAWN,
        STORED_STREAMED,
        MISSING_AT_SPAWN,
        MISSING_STREAMED,
    ];

    let server = persistent(&world).start().await;
    let alice = server.join("Alice").await;
    wait_for_chunks(&alice, &all).await;
    let stored = stored_chunk();
    for pos in [STORED_AT_SPAWN, STORED_STREAMED] {
        assert_client_chunk(&alice, pos, &stored, "the chunk stored on disk");
    }
    for pos in [MISSING_AT_SPAWN, MISSING_STREAMED] {
        assert_client_chunk(&alice, pos, &generated_chunk(pos), "generated terrain");
    }

    // Break a block of generated terrain; the shutdown flush saves its chunk.
    let broken = ground(&alice);
    let broken_chunk = (broken.x.div_euclid(16), broken.z.div_euclid(16));
    assert!(
        !all.contains(&broken_chunk),
        "the player stands in {broken_chunk:?}, one of the checked chunks"
    );
    alice
        .send(ServerboundPlayerAction {
            action: Action::StartDestroyBlock,
            pos: broken,
            direction: Direction::Up,
            seq: 1,
        })
        .await;
    alice
        .expect("the break acknowledgement", |packet| match packet {
            ClientboundGamePacket::BlockChangedAck(ack) if ack.seq == 1 => Some(()),
            _ => None,
        })
        .await;
    alice
        .wait_until("the broken block turn to air", |view| {
            view.block_at(broken).is_some_and(|state| state.is_air())
        })
        .await;
    alice.assert_healthy();
    server.stop().await;

    // A new server over the same directory loads the edited chunk instead of
    // regenerating it, and still loads the stored ones.
    let server = persistent(&world).start().await;
    let bob = server.join("Bob").await;
    wait_for_chunks(&bob, &[&all[..], &[broken_chunk]].concat()).await;
    let mut edited = generated_chunk(broken_chunk);
    edited
        .set_block(
            broken.x.rem_euclid(16) as u8,
            broken.y,
            broken.z.rem_euclid(16) as u8,
            0,
        )
        .expect("y in range");
    assert_client_chunk(&bob, broken_chunk, &edited, "the chunk saved with the edit");
    for pos in [STORED_AT_SPAWN, STORED_STREAMED] {
        assert_client_chunk(&bob, pos, &stored, "the chunk stored on disk");
    }
    for pos in [MISSING_AT_SPAWN, MISSING_STREAMED] {
        assert_client_chunk(&bob, pos, &generated_chunk(pos), "generated terrain");
    }

    bob.assert_healthy();
    server.stop().await;
}

/// A plugin moves the spawn after startup into a chunk stored on disk but
/// outside the spawn area: the join loads that chunk from the store for the
/// player standing in it instead of generating it.
#[tokio::test]
async fn a_stored_chunk_at_a_moved_spawn_is_loaded_for_the_joining_player() {
    let world = TempWorld::new();
    {
        let store = RegionChunkStore::new(world.path());
        let config = PersistenceConfig::new(world.path());
        save_chunk(
            &store,
            &config,
            DimensionId::Overworld,
            void_pos(STORED_AWAY),
            &stored_chunk(),
        )
        .expect("failed to store the chunk");
        store.flush().expect("failed to flush the store");
    }
    let calls = CallLog::default();
    let generator_calls = calls.clone();
    let server = persistent(&world)
        .config(move |config| {
            config.world_generator(RecordingGenerator {
                inner: DefaultWorldGenerator::default(),
                calls: generator_calls,
            })
        })
        .plugin(|app| {
            app.add_systems(PostStartup, |mut config: ResMut<ServerConfigResource>| {
                config.spawn_x = f64::from(STORED_AWAY.0 * 16) + 8.5;
                config.spawn_z = f64::from(STORED_AWAY.1 * 16) + 8.5;
                config.spawn_y = Some(100.0);
            });
        })
        .start()
        .await;

    let alice = server.join("Alice").await;
    wait_until_ready(&server, &alice).await;
    let own = ChunkPos::from(BlockPos::from(alice.position()));
    assert_eq!(
        own,
        client_pos(STORED_AWAY),
        "the player is at the moved spawn"
    );
    assert_client_chunk(
        &alice,
        STORED_AWAY,
        &stored_chunk(),
        "the chunk stored on disk",
    );
    let calls = calls.lock().unwrap().clone();
    assert_eq!(
        first(&calls, Call::Generate(void_pos(STORED_AWAY))),
        None,
        "the stored chunk was generated"
    );

    alice.sync().await;
    alice.assert_healthy();
    server.stop().await;
}
