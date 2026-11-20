//! VMC-ATP-009: a world change survives a controlled restart.
//!
//! A player places blocks through the use-item path, the server stops
//! cleanly (`TestServer::stop` writes `AppExit`, as `/stop` does), and the
//! same build starts again on the same world directory with
//! `voidmc-world-io`'s `WorldPersistencePlugin`. The player reconnects and the
//! chunks it receives are compared block by block with what it saw before
//! the stop. Between runs, every region file on disk is decoded.
//!
//! A chunk whose stored bytes are damaged between two runs is generated
//! again, while its damaged bytes are kept in a side file next to the region
//! file, which later saves never touch.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use bevy_ecs::prelude::{On, ResMut, Resource};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use voidmc::{
    BlockFace, ChunkDirty, ChunkIndex, ChunkLoadErrorKind, ChunkLoadFailedEvent,
    ChunkPos as VoidChunkPos, DamagedChunkAction, DimensionId, ItemBehavior, ItemBehaviorRegistry,
    ItemUseContext, ObservabilityAddress, ObservabilityConfig, UseResult,
};
use voidmc_e2e::block::{BlockState, blocks, properties::Axis};
use voidmc_e2e::core::direction::Direction;
use voidmc_e2e::core::position::{BlockPos, ChunkPos, Vec3};
use voidmc_e2e::inventory::{ItemStack, ItemStackData};
use voidmc_e2e::protocol::packets::game::{
    ClientboundGamePacket,
    s_player_action::{Action, ServerboundPlayerAction},
    s_set_creative_mode_slot::ServerboundSetCreativeModeSlot,
    s_use_item_on::{BlockHit, ServerboundUseItemOn},
};
use voidmc_e2e::registry::builtin::{BlockKind, ItemKind};
use voidmc_e2e::{Bot, LogRecord, TestServer, TestServerBuilder};
use voidmc_world_io::{ChunkStore, PersistenceConfig, RegionChunkStore, WorldPersistencePlugin};

const MIN_Y: i32 = -64;
const MAX_Y: i32 = 320;

/// Only the shutdown flush saves: no periodic save happens during a test.
const NO_PERIODIC_SAVE: u64 = u64::MAX;

/// A world directory under the system temp directory, removed on drop, also
/// when the test fails.
struct TempWorld(PathBuf);

impl TempWorld {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "voidmc-e2e-atp-009-{}-{}",
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

fn state_id(state: BlockState) -> i32 {
    u32::from(state) as i32
}

fn log(axis: Axis) -> BlockState {
    BlockState::from(blocks::OakLog { axis })
}

/// Orients a placed log along the axis of the clicked face, like vanilla.
/// VoidMC's built-in placement only places default states, so a game sets
/// orientations through an item behaviour; this one runs inside the server's
/// use-item path.
struct OrientedLog;

impl ItemBehavior for OrientedLog {
    fn on_use_on_block(&self, ctx: &mut ItemUseContext) -> UseResult {
        let Some(target) = ctx.block else {
            return UseResult::Pass;
        };
        let axis = match target.face {
            BlockFace::East | BlockFace::West => Axis::X,
            BlockFace::North | BlockFace::South => Axis::Z,
            BlockFace::Top | BlockFace::Bottom => Axis::Y,
        };
        ctx.place_block(state_id(log(axis)));
        UseResult::Handled
    }
}

/// Every run of a test starts the same build: a 3×3 spawn area around chunk
/// (0, 0), a view distance of 4 that streams the chunks around it, and the
/// persistence plugin on `world`.
fn persistent(world: &TempWorld, save_interval_ticks: u64) -> TestServerBuilder {
    let config = PersistenceConfig::new(world.path()).save_interval_ticks(save_interval_ticks);
    TestServer::builder()
        .config(|config| config.spawn_chunk_radius(1).view_distance(4))
        .plugin(move |app| {
            app.add_plugins(WorldPersistencePlugin::new(config));
            app.world_mut()
                .resource_mut::<ItemBehaviorRegistry>()
                .register_for("minecraft:oak_log", OrientedLog);
        })
}

fn void_pos((x, z): (i32, i32)) -> VoidChunkPos {
    VoidChunkPos::new(x, z)
}

fn client_pos((x, z): (i32, i32)) -> ChunkPos {
    ChunkPos::new(x, z)
}

fn chunk_of(pos: BlockPos) -> (i32, i32) {
    (pos.x.div_euclid(16), pos.z.div_euclid(16))
}

/// Every block of a chunk as state ids, in x, z, y order.
type Blocks = Vec<Option<i32>>;

fn block_positions((cx, cz): (i32, i32)) -> impl Iterator<Item = BlockPos> {
    (0..16).flat_map(move |x| {
        (0..16).flat_map(move |z| {
            (MIN_Y..MAX_Y).map(move |y| BlockPos::new(cx * 16 + x, y, cz * 16 + z))
        })
    })
}

/// The chunk at `pos` as the bot holds it.
fn seen(bot: &Bot, pos: (i32, i32)) -> Blocks {
    bot.view(|view| {
        assert!(
            view.chunks.contains_key(&client_pos(pos)),
            "{} does not hold chunk {pos:?}",
            bot.name()
        );
        block_positions(pos)
            .map(|block| view.block_at(block).map(state_id))
            .collect()
    })
}

/// The positions where two snapshots of the chunk at `pos` differ.
fn differences(pos: (i32, i32), before: &Blocks, after: &Blocks) -> BTreeSet<(i32, i32, i32)> {
    block_positions(pos)
        .zip(before.iter().zip(after))
        .filter(|(_, (before, after))| before != after)
        .map(|(block, _)| (block.x, block.y, block.z))
        .collect()
}

fn assert_same_chunk(pos: (i32, i32), expected: &Blocks, actual: &Blocks, what: &str) {
    let differ = differences(pos, expected, actual);
    assert!(
        differ.is_empty(),
        "chunk {pos:?} {what}: {} block(s) differ, first: {:?}",
        differ.len(),
        differ.iter().take(5).collect::<Vec<_>>()
    );
}

async fn wait_for_chunks(bot: &Bot, chunks: &[(i32, i32)]) {
    let wanted: Vec<ChunkPos> = chunks.iter().copied().map(client_pos).collect();
    bot.wait_until("the chunks of every site", |view| {
        wanted.iter().all(|pos| view.chunks.contains_key(pos))
    })
    .await;
    // Nothing else is in flight once the server has finished this tick.
    bot.sync().await;
}

/// Every chunk stored in the world directory, decoded, by chunk position;
/// plus the names of the region files. Panics if a region file or a chunk in
/// it cannot be read: a corrupt world fails here, not as a silent
/// regeneration.
fn stored(world: &Path) -> (BTreeSet<String>, BTreeMap<(i32, i32), Blocks>) {
    let directory = world.join("region").join("overworld");
    let store = RegionChunkStore::new(world);
    let mut files = BTreeSet::new();
    let mut chunks = BTreeMap::new();
    for entry in std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("cannot list {}: {error}", directory.display()))
    {
        let name = entry.expect("region directory entry").file_name();
        let name = name.to_string_lossy().into_owned();
        let coords: Vec<i32> = name
            .strip_prefix("r.")
            .and_then(|rest| rest.strip_suffix(".vrm"))
            .unwrap_or_else(|| panic!("unexpected file {name} in the region directory"))
            .split('.')
            .map(|part| part.parse().expect("region coordinate"))
            .collect();
        let [rx, rz] = coords[..] else {
            panic!("unexpected region file name {name}");
        };
        for x in rx * 32..rx * 32 + 32 {
            for z in rz * 32..rz * 32 + 32 {
                let Some(bytes) = store
                    .read(DimensionId::Overworld, void_pos((x, z)))
                    .unwrap_or_else(|error| panic!("{name}: chunk ({x}, {z}) unreadable: {error}"))
                else {
                    continue;
                };
                let chunk = voidmc_world_io::format::deserialize_chunk(&bytes)
                    .unwrap_or_else(|error| panic!("{name}: chunk ({x}, {z}) undecodable: {error}"))
                    .data;
                let blocks = (0..16u8)
                    .flat_map(|lx| (0..16u8).map(move |lz| (lx, lz)))
                    .flat_map(|(lx, lz)| (MIN_Y..MAX_Y).map(move |y| (lx, y, lz)))
                    .map(|(lx, y, lz)| chunk.get_block(lx, y, lz))
                    .collect();
                chunks.insert((x, z), blocks);
            }
        }
        files.insert(name);
    }
    (files, chunks)
}

/// The highest non-air block of a column, as the bot sees it.
fn top(bot: &Bot, x: i32, z: i32) -> BlockPos {
    bot.view(|view| view.ground_below(BlockPos::new(x, MAX_Y - 1, z)))
        .unwrap_or_else(|| panic!("{} holds no column at ({x}, {z})", bot.name()))
}

/// Uses `item` on `face` of `against`. A click only reaches blocks a few
/// blocks away, so the bot first stands four blocks above the clicked block,
/// out of the way of what it places, and goes back where it was afterwards:
/// at the start position every site's chunk is in view.
async fn use_item_on(bot: &Bot, item: ItemKind, against: BlockPos, face: Direction, seq: u32) {
    let home = bot.position();
    let above = Vec3::new(
        f64::from(against.x) + 0.5,
        f64::from(against.y + 4),
        f64::from(against.z) + 0.5,
    );
    bot.move_to(above).await;
    bot.send(ServerboundSetCreativeModeSlot {
        slot_num: 36,
        item_stack: ItemStack::Present(ItemStackData::new(item, 1)),
    })
    .await;
    let normal = face.normal();
    bot.send(ServerboundUseItemOn {
        hand: Default::default(),
        block_hit: BlockHit {
            block_pos: against,
            direction: face,
            location: Vec3::new(
                f64::from(against.x) + 0.5 + f64::from(normal.x) * 0.5,
                f64::from(against.y) + 0.5 + f64::from(normal.y) * 0.5,
                f64::from(against.z) + 0.5 + f64::from(normal.z) * 0.5,
            ),
            inside: false,
            world_border: false,
        },
        seq,
    })
    .await;
    bot.expect("the placement acknowledgement", |packet| match packet {
        ClientboundGamePacket::BlockChangedAck(ack) if ack.seq == seq => Some(()),
        _ => None,
    })
    .await;
    bot.move_to(home).await;
}

/// A recorded change: a block placed at a coordinate with an exact state.
#[derive(Clone, Copy, Debug)]
struct Placed {
    pos: BlockPos,
    state: BlockState,
}

const DIAMOND: (ItemKind, BlockKind) = (ItemKind::DiamondBlock, BlockKind::DiamondBlock);
const GOLD: (ItemKind, BlockKind) = (ItemKind::GoldBlock, BlockKind::GoldBlock);
const EMERALD: (ItemKind, BlockKind) = (ItemKind::EmeraldBlock, BlockKind::EmeraldBlock);
const LAPIS: (ItemKind, BlockKind) = (ItemKind::LapisBlock, BlockKind::LapisBlock);

/// Places `block` on top of the highest block of the column at `(x, z)`
/// within `chunk`.
async fn place_on_top(
    bot: &Bot,
    chunk: (i32, i32),
    (x, z): (i32, i32),
    block: (ItemKind, BlockKind),
    seq: u32,
) -> Placed {
    let ground = top(bot, chunk.0 * 16 + x, chunk.1 * 16 + z);
    let pos = ground.up(1);
    let state = BlockState::from(block.1);
    use_item_on(bot, block.0, ground, Direction::Up, seq).await;
    bot.wait_until("the placed block", |view| view.block_at(pos) == Some(state))
        .await;
    Placed { pos, state }
}

/// Places `base` on top of the highest block of a column of `chunk`, then an
/// oak log against the side of `base` facing `side`, so the log lies along
/// that side's axis: a non-default state. Returns both placements.
async fn build_site(
    bot: &Bot,
    chunk: (i32, i32),
    base: (ItemKind, BlockKind),
    side: Direction,
    seq: u32,
) -> [Placed; 2] {
    let step = side.normal();
    // A column whose neighbour on `side`, in the same chunk, is not higher:
    // the block next to `base` is then air. Columns 0 and 1 are skipped so
    // that nothing is placed where the player stands at spawn. A column
    // topped with water is skipped too: a block placed on water replaces it.
    let (x, z) = (2..15)
        .flat_map(|a| (2..15).map(move |b| (a, b)))
        .find(|&(a, b)| {
            let (x, z) = (chunk.0 * 16 + a, chunk.1 * 16 + b);
            let ground = top(bot, x, z);
            let water = bot.block_at(ground).map(BlockKind::from) == Some(BlockKind::Water);
            !water && top(bot, x + step.x, z + step.z).y <= ground.y
        })
        .unwrap_or_else(|| panic!("no column of chunk {chunk:?} fits a log on its {side:?} side"));
    let base = place_on_top(bot, chunk, (x, z), base, seq).await;
    let log_pos = BlockPos::new(base.pos.x + step.x, base.pos.y, base.pos.z + step.z);
    assert_eq!(chunk_of(log_pos), chunk);
    assert!(
        bot.block_at(log_pos).is_some_and(|state| state.is_air()),
        "{log_pos:?} is not air before the placement"
    );

    let axis = match side {
        Direction::East | Direction::West => Axis::X,
        Direction::North | Direction::South => Axis::Z,
        Direction::Up | Direction::Down => Axis::Y,
    };
    let log_state = log(axis);
    assert_ne!(
        log_state,
        BlockState::from(BlockKind::OakLog),
        "the log state must not be the default one"
    );
    use_item_on(bot, ItemKind::OakLog, base.pos, side, seq + 1).await;
    bot.wait_until("the placed oriented log", |view| {
        view.block_at(log_pos) == Some(log_state)
    })
    .await;

    [
        base,
        Placed {
            pos: log_pos,
            state: log_state,
        },
    ]
}

struct Site {
    chunk: (i32, i32),
    base: (ItemKind, BlockKind),
    side: Direction,
    region: &'static str,
}

/// Two sites in the pre-generated spawn area and two streamed ones, each in
/// its own region file, on both sides of the region boundary at x = 0 and
/// z = 0.
const SITES: [Site; 4] = [
    Site {
        chunk: (0, 0),
        base: DIAMOND,
        side: Direction::East,
        region: "r.0.0.vrm",
    },
    Site {
        chunk: (-1, -1),
        base: GOLD,
        side: Direction::South,
        region: "r.-1.-1.vrm",
    },
    Site {
        chunk: (3, -2),
        base: EMERALD,
        side: Direction::South,
        region: "r.0.-1.vrm",
    },
    Site {
        chunk: (-3, 2),
        base: LAPIS,
        side: Direction::East,
        region: "r.-1.0.vrm",
    },
];

#[tokio::test]
async fn placed_blocks_keep_their_state_and_surroundings_across_two_restarts() {
    let world = TempWorld::new();
    let chunks: Vec<(i32, i32)> = SITES.iter().map(|site| site.chunk).collect();

    // First run: place a base block and an oriented log at every site.
    let server = persistent(&world, NO_PERIODIC_SAVE).start().await;
    let at_startup: Vec<bool> = server
        .with_world({
            let chunks = chunks.clone();
            move |world| {
                let index = world.resource::<ChunkIndex>();
                chunks
                    .iter()
                    .map(|pos| {
                        index
                            .0
                            .contains_key(&(DimensionId::Overworld, void_pos(*pos)))
                    })
                    .collect()
            }
        })
        .await;
    assert_eq!(
        at_startup,
        [true, true, false, false],
        "the first two sites are in the spawn area, the last two are streamed"
    );

    let alice = server.join("Alice").await;
    wait_for_chunks(&alice, &chunks).await;
    let before: Vec<Blocks> = chunks.iter().map(|pos| seen(&alice, *pos)).collect();
    let mut placed = Vec::new();
    for (index, site) in SITES.iter().enumerate() {
        let seq = 1 + 2 * index as u32;
        // Back at the start after the previous site, every site is in view
        // again once its chunks are resent.
        wait_for_chunks(&alice, &chunks).await;
        placed.push(build_site(&alice, site.chunk, site.base, site.side, seq).await);
    }
    wait_for_chunks(&alice, &chunks).await;
    let expected: Vec<Blocks> = chunks.iter().map(|pos| seen(&alice, *pos)).collect();
    for ((pos, before), (after, placed)) in
        chunks.iter().zip(&before).zip(expected.iter().zip(&placed))
    {
        let wanted: BTreeSet<_> = placed.iter().map(|p| (p.pos.x, p.pos.y, p.pos.z)).collect();
        assert_eq!(
            differences(*pos, before, after),
            wanted,
            "only the placed blocks changed in chunk {pos:?}"
        );
    }
    alice.assert_healthy();
    server.stop().await;

    // The shutdown flush wrote every changed chunk, each to its region file.
    let (files, saved) = stored(world.path());
    for site in &SITES {
        assert!(
            files.contains(site.region),
            "{} missing after the first run: {files:?}",
            site.region
        );
    }
    for (pos, expected) in chunks.iter().zip(&expected) {
        let on_disk = saved
            .get(pos)
            .unwrap_or_else(|| panic!("chunk {pos:?} was not saved on stop"));
        assert_same_chunk(*pos, expected, on_disk, "on disk after the first run");
    }

    // Second run changes nothing, third run checks the second did no harm.
    for run in ["second", "third"] {
        let server = persistent(&world, NO_PERIODIC_SAVE).start().await;
        let alice = server.join("Alice").await;
        wait_for_chunks(&alice, &chunks).await;
        for block in placed.iter().flatten() {
            assert_eq!(
                alice.block_at(block.pos),
                Some(block.state),
                "the block recorded at {:?} after the {run} start",
                block.pos
            );
        }
        for (pos, expected) in chunks.iter().zip(&expected) {
            let actual = seen(&alice, *pos);
            assert_same_chunk(*pos, expected, &actual, &format!("after the {run} start"));
        }
        alice.assert_healthy();
        server.stop().await;

        let (after_files, after) = stored(world.path());
        assert!(
            after_files.is_superset(&files),
            "region files lost after the {run} run: {files:?} became {after_files:?}"
        );
        for (pos, blocks) in &saved {
            let now = after
                .get(pos)
                .unwrap_or_else(|| panic!("chunk {pos:?} vanished after the {run} run"));
            assert_same_chunk(*pos, blocks, now, &format!("on disk after the {run} run"));
        }
    }
}

/// Whether the server holds the chunk at `pos` with unsaved changes.
async fn is_dirty(server: &TestServer, pos: (i32, i32)) -> bool {
    server
        .with_world(move |world| {
            let entity = world.resource::<ChunkIndex>().0[&(DimensionId::Overworld, void_pos(pos))];
            world.get::<ChunkDirty>(entity).is_some()
        })
        .await
}

#[tokio::test]
async fn a_change_reverted_after_a_save_stays_reverted_after_restart() {
    let world = TempWorld::new();
    let chunk = (2, -3);

    // Save every tick, so the placed block reaches the disk before the revert.
    let server = persistent(&world, 1).start().await;
    let alice = server.join("Alice").await;
    wait_for_chunks(&alice, &[chunk]).await;
    let original = seen(&alice, chunk);

    let placed = place_on_top(&alice, chunk, (7, 7), DIAMOND, 1).await.pos;
    alice.sync().await;
    assert!(
        !is_dirty(&server, chunk).await,
        "the periodic save did not save the chunk"
    );
    // With nothing dirty the server does not write, so the files can be read.
    let (_, saved) = stored(world.path());
    let with_block = seen(&alice, chunk);
    assert_eq!(
        differences(chunk, &original, &with_block),
        BTreeSet::from([(placed.x, placed.y, placed.z)])
    );
    assert_same_chunk(
        chunk,
        &with_block,
        saved.get(&chunk).expect("the chunk is on disk"),
        "on disk before the revert",
    );

    // Alice breaks it from above, within reach, then goes back.
    let home = alice.position();
    let above = Vec3::new(
        f64::from(placed.x) + 0.5,
        f64::from(placed.y + 3),
        f64::from(placed.z) + 0.5,
    );
    alice.move_to(above).await;
    alice
        .send(ServerboundPlayerAction {
            action: Action::StartDestroyBlock,
            pos: placed,
            direction: Direction::Up,
            seq: 2,
        })
        .await;
    alice
        .expect("the break acknowledgement", |packet| match packet {
            ClientboundGamePacket::BlockChangedAck(ack) if ack.seq == 2 => Some(()),
            _ => None,
        })
        .await;
    alice
        .wait_until("the placed block removed", |view| {
            view.block_at(placed).is_some_and(|state| state.is_air())
        })
        .await;
    alice.move_to(home).await;
    wait_for_chunks(&alice, &[chunk]).await;
    assert_same_chunk(chunk, &original, &seen(&alice, chunk), "after the revert");
    alice.assert_healthy();
    server.stop().await;

    let (_, saved) = stored(world.path());
    assert_same_chunk(
        chunk,
        &original,
        saved.get(&chunk).expect("the chunk is on disk"),
        "on disk after the stop",
    );

    let server = persistent(&world, 1).start().await;
    let alice = server.join("Alice").await;
    wait_for_chunks(&alice, &[chunk]).await;
    assert!(
        alice.block_at(placed).is_some_and(|state| state.is_air()),
        "the reverted block is back after the restart"
    );
    assert_same_chunk(chunk, &original, &seen(&alice, chunk), "after the restart");
    alice.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn a_later_session_writes_into_a_region_file_without_damaging_it() {
    let world = TempWorld::new();
    // Both chunks are in r.0.0.vrm.
    let (first, second) = ((0, 0), (1, 0));

    // First run: one change in the first chunk.
    let server = persistent(&world, NO_PERIODIC_SAVE).start().await;
    let alice = server.join("Alice").await;
    wait_for_chunks(&alice, &[first, second]).await;
    let early = place_on_top(&alice, first, (3, 3), DIAMOND, 1).await;
    alice.assert_healthy();
    server.stop().await;

    // Second run: the region file is opened from disk and a chunk it does not
    // hold yet is added to it.
    let server = persistent(&world, NO_PERIODIC_SAVE).start().await;
    let alice = server.join("Alice").await;
    wait_for_chunks(&alice, &[first, second]).await;
    assert_eq!(alice.block_at(early.pos), Some(early.state));
    let late = place_on_top(&alice, second, (8, 8), EMERALD, 1).await;
    wait_for_chunks(&alice, &[first, second]).await;
    let expected = [seen(&alice, first), seen(&alice, second)];
    alice.assert_healthy();
    server.stop().await;

    let (files, saved) = stored(world.path());
    assert_eq!(files, BTreeSet::from(["r.0.0.vrm".to_string()]));
    for (pos, expected) in [first, second].into_iter().zip(&expected) {
        let on_disk = saved
            .get(&pos)
            .unwrap_or_else(|| panic!("chunk {pos:?} was not saved"));
        assert_same_chunk(pos, expected, on_disk, "on disk after the second run");
    }

    // Third run: both sessions' changes are served.
    let server = persistent(&world, NO_PERIODIC_SAVE).start().await;
    let alice = server.join("Alice").await;
    wait_for_chunks(&alice, &[first, second]).await;
    for block in [early, late] {
        assert_eq!(alice.block_at(block.pos), Some(block.state), "{block:?}");
    }
    for (pos, expected) in [first, second].into_iter().zip(&expected) {
        assert_same_chunk(pos, expected, &seen(&alice, pos), "after the third start");
    }
    alice.assert_healthy();
    server.stop().await;
}

// --- A chunk whose stored bytes are damaged ------------------------------

/// The chunk load failures the server triggered, in order.
#[derive(Resource, Default)]
struct LoadFailures(Vec<ChunkLoadFailedEvent>);

/// [`persistent`] without periodic saves, with the metrics endpoint on a free
/// port, recording [`ChunkLoadFailedEvent`]s in [`LoadFailures`].
fn persistent_observed(world: &TempWorld) -> TestServerBuilder {
    persistent(world, NO_PERIODIC_SAVE)
        .config(|config| config.observability(ObservabilityConfig::localhost(0)))
        .plugin(|app| {
            app.init_resource::<LoadFailures>().add_observer(
                |event: On<ChunkLoadFailedEvent>, mut failures: ResMut<LoadFailures>| {
                    failures.0.push(event.event().clone());
                },
            );
        })
}

async fn load_failures(server: &TestServer) -> Vec<ChunkLoadFailedEvent> {
    server
        .with_world(|world| world.resource::<LoadFailures>().0.clone())
        .await
}

/// Checks that the only load failure of the run is the decoding of the
/// chunk at `pos`, quarantined in the side file `name`.
async fn assert_quarantined(server: &TestServer, world: &Path, pos: (i32, i32), name: &str) {
    let failures = load_failures(server).await;
    assert_eq!(failures.len(), 1, "{failures:#?}");
    let failure = &failures[0];
    assert_eq!(
        (
            failure.dimension,
            failure.pos,
            failure.kind,
            &failure.action
        ),
        (
            DimensionId::Overworld,
            void_pos(pos),
            ChunkLoadErrorKind::Decode,
            &DamagedChunkAction::Quarantined(world.join("region").join("overworld").join(name)),
        )
    );
}

const SECTOR: usize = 4096;

fn region_file(world: &Path, (cx, cz): (i32, i32)) -> PathBuf {
    world
        .join("region")
        .join("overworld")
        .join(format!("r.{}.{}.vrm", cx >> 5, cz >> 5))
}

/// The byte range of the sectors that hold the chunk at `pos` in its region
/// file, read from the file's location table.
fn slot(file: &[u8], (cx, cz): (i32, i32)) -> Range<usize> {
    let index = ((cx & 31) + (cz & 31) * 32) as usize * 4;
    let location = u32::from_be_bytes(file[index..index + 4].try_into().unwrap());
    assert_ne!(location, 0, "chunk ({cx}, {cz}) is not stored");
    let offset = (location >> 8) as usize * SECTOR;
    offset..offset + (location & 0xFF) as usize * SECTOR
}

/// Damages the compressed data of the chunk at `pos` in place, as bit rot
/// would, leaving its length and compression byte intact. Returns the
/// sectors of the chunk as they now are on disk.
fn damage(world: &Path, pos: (i32, i32), pattern: u8) -> Vec<u8> {
    let path = region_file(world, pos);
    let mut file = std::fs::read(&path).expect("the region file is readable");
    let range = slot(&file, pos);
    let length = u32::from_be_bytes(file[range.start..range.start + 4].try_into().unwrap());
    // `length` counts the compression byte, which is left as it is.
    for byte in &mut file[range.start + 5..range.start + 4 + length as usize] {
        *byte ^= pattern;
    }
    std::fs::write(&path, &file).expect("the region file is writable");
    file[range].to_vec()
}

/// The side files of the overworld region directory, by name, with their
/// bytes.
fn side_files(world: &Path) -> BTreeMap<String, Vec<u8>> {
    let directory = world.join("region").join("overworld");
    std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("cannot list {}: {error}", directory.display()))
        .map(|entry| entry.expect("region directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "corrupt"))
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (
                name,
                std::fs::read(&path).expect("the side file is readable"),
            )
        })
        .collect()
}

/// The value of `voidmc_chunk_load_errors_total{kind}` on the metrics
/// endpoint, `None` without such a sample.
async fn chunk_load_errors(server: &TestServer, kind: &str) -> Option<f64> {
    let address = server
        .with_world(|world| world.resource::<ObservabilityAddress>().0)
        .await;
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream
        .write_all(b"GET /metrics HTTP/1.1\r\nHost: test\r\n\r\n")
        .await
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    let prefix = format!("voidmc_chunk_load_errors_total{{kind=\"{kind}\"}} ");
    response
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .map(|value| value.parse().expect("a sample value"))
}

/// The chunk at `pos` as stored on disk, decoded; panics if it cannot be.
fn stored_chunk(world: &Path, pos: (i32, i32)) -> voidmc::ChunkData {
    let bytes = RegionChunkStore::new(world)
        .read(DimensionId::Overworld, void_pos(pos))
        .unwrap_or_else(|error| panic!("chunk {pos:?} unreadable: {error}"))
        .unwrap_or_else(|| panic!("chunk {pos:?} is not stored"));
    voidmc_world_io::format::deserialize_chunk(&bytes)
        .unwrap_or_else(|error| panic!("chunk {pos:?} undecodable: {error}"))
        .data
}

/// The error `voidmc-world-io` logs for a stored chunk it cannot load.
fn is_chunk_load_error(record: &LogRecord) -> bool {
    record.target.starts_with("voidmc_world_io") && record.message.contains("chunk")
}

#[tokio::test]
async fn a_chunk_that_cannot_be_decoded_is_quarantined_then_generated_again() {
    let world = TempWorld::new();
    // Both chunks are in r.0.0.vrm, in the spawn area loaded at startup.
    let (intact, damaged) = ((0, 0), (1, 0));

    // First run: a change in each chunk, saved on stop.
    let server = persistent_observed(&world).start().await;
    let alice = server.join("Alice").await;
    wait_for_chunks(&alice, &[intact, damaged]).await;
    let generated = seen(&alice, damaged);
    place_on_top(&alice, intact, (3, 3), DIAMOND, 1).await;
    place_on_top(&alice, damaged, (5, 5), GOLD, 2).await;
    alice.sync().await;
    let built = seen(&alice, intact);
    alice.assert_healthy();
    assert!(load_failures(&server).await.is_empty());
    server.stop().await;

    let first_damage = damage(world.path(), damaged, 0x5A);

    // Second run: the damaged chunk is generated again, its bytes are in a
    // side file and the failure is counted; the other chunk loads as saved.
    let server = persistent_observed(&world).start().await;
    let alice = server.join("Alice").await;
    wait_for_chunks(&alice, &[intact, damaged]).await;
    assert_same_chunk(
        damaged,
        &generated,
        &seen(&alice, damaged),
        "after the damage",
    );
    assert_same_chunk(
        intact,
        &built,
        &seen(&alice, intact),
        "next to the damaged one",
    );
    let first_side = side_files(world.path());
    assert_eq!(
        first_side.keys().collect::<Vec<_>>(),
        ["c.1.0.1.corrupt"],
        "one side file for the damaged chunk"
    );
    assert!(
        first_side["c.1.0.1.corrupt"] == first_damage,
        "the side file does not hold the damaged sectors"
    );
    assert_eq!(chunk_load_errors(&server, "decode").await, Some(1.0));
    assert_eq!(chunk_load_errors(&server, "io").await, Some(0.0));
    assert_quarantined(&server, world.path(), damaged, "c.1.0.1.corrupt").await;

    // An edit in the generated chunk is saved over the damaged slot; the side
    // file is left as it was.
    let edit = place_on_top(&alice, damaged, (8, 8), EMERALD, 1).await;
    alice.sync().await;
    alice.assert_healthy();
    let failures = server.stop_expecting(is_chunk_load_error).await;
    assert_eq!(failures.len(), 1, "{failures:#?}");
    assert_eq!(
        side_files(world.path()),
        first_side,
        "after a save of the chunk"
    );
    let (lx, lz) = (
        edit.pos.x.rem_euclid(16) as u8,
        edit.pos.z.rem_euclid(16) as u8,
    );
    assert_eq!(
        stored_chunk(world.path(), damaged).get_block(lx, edit.pos.y, lz),
        Some(state_id(edit.state)),
        "the edit made after the damage is saved"
    );

    // A second damage of the same chunk gets a side file of its own.
    let second_damage = damage(world.path(), damaged, 0xA5);
    let server = persistent_observed(&world).start().await;
    let alice = server.join("Alice").await;
    wait_for_chunks(&alice, &[intact, damaged]).await;
    assert_same_chunk(
        damaged,
        &generated,
        &seen(&alice, damaged),
        "after the second damage",
    );
    let second_side = side_files(world.path());
    assert_eq!(
        second_side.keys().collect::<Vec<_>>(),
        ["c.1.0.1.corrupt", "c.1.0.2.corrupt"]
    );
    assert!(
        second_side["c.1.0.1.corrupt"] == first_damage,
        "the first side file changed"
    );
    assert!(
        second_side["c.1.0.2.corrupt"] == second_damage,
        "the second side file does not hold the damaged sectors"
    );
    assert_eq!(chunk_load_errors(&server, "decode").await, Some(1.0));
    alice.assert_healthy();
    assert_quarantined(&server, world.path(), damaged, "c.1.0.2.corrupt").await;
    let failures = server.stop_expecting(is_chunk_load_error).await;
    assert_eq!(failures.len(), 1, "{failures:#?}");
    assert_eq!(side_files(world.path()), second_side, "after the third run");
}
