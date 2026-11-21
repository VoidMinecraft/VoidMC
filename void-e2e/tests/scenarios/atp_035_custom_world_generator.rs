//! VMC-ATP-035: an application replaces the default terrain generator with a
//! recognizable custom one.
//!
//! The generator below lives entirely in this crate and uses only the public
//! API of `voidmc` and `voidmc-protocol`, like an application would. Its
//! pattern is defined on world coordinates and differs along x and z, so a
//! chunk generated for the wrong column, with chunk-local coordinates or with
//! x and z swapped, does not match. Its surface stands well above the default
//! sine terrain and varies from one column to the next, so a spawn computed
//! from the wrong generator, the wrong column or off by one block is caught.
//! It also emits what `DefaultWorldGenerator` never does: non-default block
//! states, sections with more than 256 distinct block states, and biomes
//! other than plains, in indirect and direct palettes.

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};

use voidmc::{BiomeId, ChunkIndex, ChunkPos as VoidChunkPos, SpawnPosition, WorldGenerator};
use voidmc_e2e::block::BlockState;
use voidmc_e2e::block::blocks::OakLog;
use voidmc_e2e::block::properties::Axis;
use voidmc_e2e::core::position::{BlockPos, ChunkBlockPos, ChunkPos, ChunkSectionBiomePos, Vec3};
use voidmc_e2e::protocol::packets::game::ClientboundGamePacket;
use voidmc_e2e::registry::builtin::BlockKind;
use voidmc_e2e::{Bot, BotOptions, TestServer};
use voidmc_protocol::clientbound::chunk::{Chunk, ChunkBuilder};

const MIN_Y: i32 = -64;
const MAX_Y: i32 = 319;

// ---------------------------------------------------------------------------
// The custom generator, as an application would write it.
// ---------------------------------------------------------------------------

/// First air block above column `(x, z)`: from 100 to 108, well above the
/// default terrain (at most Y 85), and different along x and z.
fn surface(x: i32, z: i32) -> i32 {
    100 + x.rem_euclid(5) + 2 * z.rem_euclid(3)
}

const WOOL: [BlockKind; 16] = [
    BlockKind::WhiteWool,
    BlockKind::OrangeWool,
    BlockKind::MagentaWool,
    BlockKind::LightBlueWool,
    BlockKind::YellowWool,
    BlockKind::LimeWool,
    BlockKind::PinkWool,
    BlockKind::GrayWool,
    BlockKind::LightGrayWool,
    BlockKind::CyanWool,
    BlockKind::PurpleWool,
    BlockKind::BlueWool,
    BlockKind::BrownWool,
    BlockKind::GreenWool,
    BlockKind::RedWool,
    BlockKind::BlackWool,
];

/// Lowest state id of the noise layer, and how many consecutive ids it uses.
/// None of them is stone, dirt, grass or water.
const NOISE_FIRST: u32 = 1000;
const NOISE_IDS: u32 = 1500;

fn noise(x: i32, y: i32, z: i32) -> u32 {
    let n = (x as u32).wrapping_mul(73_856_093)
        ^ (y as u32).wrapping_mul(19_349_663)
        ^ (z as u32).wrapping_mul(83_492_791);
    NOISE_FIRST + n % NOISE_IDS
}

/// The block at world `(x, y, z)`, from the top down: air, a wool tile whose
/// colour follows 2×2 tiles of the world grid, an oak log whose axis cycles
/// with the column (two of its three states are not the default one),
/// terracotta, and in the bottom section a noise of ~1,400 distinct states.
fn pattern(x: i32, y: i32, z: i32) -> BlockState {
    let top = surface(x, z);
    if y >= top {
        BlockState::AIR
    } else if y == top - 1 {
        WOOL[(x.div_euclid(2) + 3 * z.div_euclid(2)).rem_euclid(16) as usize].into()
    } else if y == top - 2 {
        let axis = [Axis::X, Axis::Y, Axis::Z][(x + 2 * z).rem_euclid(3) as usize];
        OakLog { axis }.into()
    } else if y < MIN_Y + 16 {
        BlockState::try_from(noise(x, y, z)).expect("a valid state id")
    } else {
        BlockKind::Terracotta.into()
    }
}

const BIOMES: [&str; 10] = [
    "minecraft:desert",
    "minecraft:ocean",
    "minecraft:badlands",
    "minecraft:beach",
    "minecraft:jungle",
    "minecraft:snowy_plains",
    "minecraft:savanna",
    "minecraft:swamp",
    "minecraft:taiga",
    "minecraft:mushroom_fields",
];

/// Biome name of the 4×4×4 cell whose lowest corner is at `(x, y, z)`. Below
/// Y 0 three biomes alternate (an indirect palette); above, all ten appear in
/// every section (a direct palette). Plains, the default, never does.
fn biome_name(x: i32, y: i32, z: i32) -> &'static str {
    let (cx, cy, cz) = (x.div_euclid(4), y.div_euclid(4), z.div_euclid(4));
    if y < 0 {
        BIOMES[(cx + cz).rem_euclid(3) as usize]
    } else {
        BIOMES[(cx + 2 * cz + 3 * cy).rem_euclid(10) as usize]
    }
}

fn biome(x: i32, y: i32, z: i32) -> BiomeId {
    BiomeId::named(biome_name(x, y, z)).expect("a vanilla biome")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Call {
    Generated(VoidChunkPos),
    Surface(i32, i32),
}

type CallLog = Arc<Mutex<Vec<Call>>>;

/// The custom generator, recording every call the server makes to it.
struct Tiles {
    calls: CallLog,
}

impl WorldGenerator for Tiles {
    fn generate_chunk(&self, pos: &VoidChunkPos) -> Chunk {
        self.calls.lock().unwrap().push(Call::Generated(*pos));
        let mut builder =
            ChunkBuilder::new(pos.x, pos.z).biomes_from(|x, y, z| self.cell_biome(x, y, z).0);
        for local_z in 0..16u8 {
            for local_x in 0..16u8 {
                let x = pos.x * 16 + i32::from(local_x);
                let z = pos.z * 16 + i32::from(local_z);
                for y in MIN_Y..surface(x, z) {
                    let state = u32::from(pattern(x, y, z)) as i32;
                    builder.set_block(local_x, y, local_z, state);
                }
            }
        }
        builder.build()
    }

    fn surface_height_at(&self, block_x: i32, block_z: i32) -> i32 {
        self.calls
            .lock()
            .unwrap()
            .push(Call::Surface(block_x, block_z));
        surface(block_x, block_z)
    }

    fn cell_biome(&self, block_x: i32, block_y: i32, block_z: i32) -> BiomeId {
        biome(block_x, block_y, block_z)
    }
}

// ---------------------------------------------------------------------------
// Checks on what the client received.
// ---------------------------------------------------------------------------

/// A spawn away from the origin, with negative z, in the middle of a block:
/// the spawn column is `(37, -22)` (floor), not `(37, -21)` (truncation) nor
/// `(38, -21)` (rounding), and their surfaces all differ.
const SPAWN_X: f64 = 37.5;
const SPAWN_Z: f64 = -21.5;
const SPAWN_COLUMN: (i32, i32) = (37, -22);

const VIEW_DISTANCE: u8 = 3;

fn square(center: ChunkPos, radius: i32) -> HashSet<ChunkPos> {
    (-radius..=radius)
        .flat_map(|dx| {
            (-radius..=radius).map(move |dz| ChunkPos::new(center.x + dx, center.z + dz))
        })
        .collect()
}

fn client_pos(pos: VoidChunkPos) -> ChunkPos {
    ChunkPos::new(pos.x, pos.z)
}

fn generated(calls: &CallLog) -> Vec<ChunkPos> {
    calls
        .lock()
        .unwrap()
        .iter()
        .filter_map(|call| match call {
            Call::Generated(pos) => Some(client_pos(*pos)),
            Call::Surface(..) => None,
        })
        .collect()
}

fn surface_calls(calls: &CallLog) -> Vec<(i32, i32)> {
    calls
        .lock()
        .unwrap()
        .iter()
        .filter_map(|call| match call {
            Call::Surface(x, z) => Some((*x, *z)),
            Call::Generated(_) => None,
        })
        .collect()
}

async fn indexed_chunks(server: &TestServer) -> HashSet<ChunkPos> {
    server
        .with_world(|world| {
            world
                .resource::<ChunkIndex>()
                .0
                .keys()
                .map(|(_, pos)| client_pos(*pos))
                .collect()
        })
        .await
}

fn loaded(bot: &Bot) -> HashSet<ChunkPos> {
    bot.view(|view| view.chunks.keys().copied().collect())
}

fn note(mismatches: &mut Vec<String>, mismatch: impl FnOnce() -> String) {
    if mismatches.len() < 30 {
        mismatches.push(mismatch());
    }
}

/// Compares every block and every biome cell of every chunk the client holds
/// with the pattern, and checks that no block of the default terrain appears.
/// Returns how many chunks were compared.
fn assert_pattern(bot: &Bot, when: &str) -> usize {
    let biome_ids: Vec<(&str, u32)> = BIOMES
        .iter()
        .map(|name| (*name, BiomeId::named(name).unwrap().0 as u32))
        .collect();
    let default_terrain = [
        BlockKind::Stone,
        BlockKind::Dirt,
        BlockKind::GrassBlock,
        BlockKind::Water,
    ];
    bot.view(|view| {
        let mut mismatches = Vec::new();
        for (pos, chunk) in &view.chunks {
            for z in 0..16u8 {
                for x in 0..16u8 {
                    let (wx, wz) = (pos.x * 16 + i32::from(x), pos.z * 16 + i32::from(z));
                    for y in MIN_Y..=MAX_Y {
                        let held = chunk.get_block_state(&ChunkBlockPos::new(x, y, z), MIN_Y);
                        let expected = pattern(wx, y, wz);
                        if held != Some(expected) {
                            note(&mut mismatches, || format!(
                                "block ({wx}, {y}, {wz}): {held:?} instead of {expected:?}"
                            ));
                        }
                        if let Some(held) = held {
                            assert!(
                                !default_terrain.contains(&BlockKind::from(held)),
                                "{when}: default terrain at ({wx}, {y}, {wz}): {held:?}"
                            );
                        }
                    }
                }
            }
            for (section_index, section) in chunk.sections.iter().enumerate() {
                for cell_y in 0..4u8 {
                    for cell_z in 0..4u8 {
                        for cell_x in 0..4u8 {
                            let wx = pos.x * 16 + i32::from(cell_x) * 4;
                            let wy = MIN_Y + section_index as i32 * 16 + i32::from(cell_y) * 4;
                            let wz = pos.z * 16 + i32::from(cell_z) * 4;
                            let held = u32::from(section.get_biome(ChunkSectionBiomePos {
                                x: cell_x,
                                y: cell_y,
                                z: cell_z,
                            }));
                            let name = biome_name(wx, wy, wz);
                            let expected = biome_ids.iter().find(|(n, _)| *n == name).unwrap().1;
                            if held != expected {
                                note(&mut mismatches, || format!(
                                    "biome cell ({wx}, {wy}, {wz}): {held} instead of {name} ({expected})"
                                ));
                            }
                        }
                    }
                }
            }
        }
        assert!(
            mismatches.is_empty(),
            "{when}: the client's terrain differs from the custom generator, first differences:\n{}",
            mismatches.join("\n")
        );
        view.chunks.len()
    })
}

/// Where the server placed the bot when it joined.
async fn spawn_teleport(bot: &Bot) -> Vec3 {
    bot.expect("the spawn teleport", |packet| match packet {
        ClientboundGamePacket::PlayerPosition(teleport) => Some(teleport.change.pos),
        _ => None,
    })
    .await
}

/// Every chunk packet the bot received, by position, in order.
fn chunk_packets(bot: &Bot) -> BTreeMap<(i32, i32), Vec<String>> {
    let mut sent: BTreeMap<(i32, i32), Vec<String>> = BTreeMap::new();
    for (pos, packet) in bot.queued(|packet| match packet {
        ClientboundGamePacket::LevelChunkWithLight(chunk) => {
            Some(((chunk.x, chunk.z), format!("{chunk:?}")))
        }
        _ => None,
    }) {
        sent.entry(pos).or_default().push(packet);
    }
    sent
}

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

#[tokio::test]
async fn custom_generator_shapes_the_spawn_and_every_streamed_chunk() {
    let calls = CallLog::default();
    let generator_calls = calls.clone();
    let server = TestServer::builder()
        .config(move |config| {
            config
                .view_distance(i32::from(VIEW_DISTANCE))
                .spawn_chunk_radius(1)
                .spawn_position(SpawnPosition {
                    x: SPAWN_X,
                    z: SPAWN_Z,
                    y: None,
                })
                .world_generator(Tiles {
                    calls: generator_calls,
                })
        })
        .start()
        .await;

    // The spawn area was pre-generated by the custom generator, around the
    // configured spawn rather than the origin.
    let spawn_chunk = ChunkPos::new(2, -2);
    let spawn_area = square(spawn_chunk, 1);
    assert_eq!(indexed_chunks(&server).await, spawn_area);
    let startup = generated(&calls);
    assert_eq!(startup.len(), spawn_area.len(), "{startup:?}");
    assert_eq!(startup.iter().copied().collect::<HashSet<_>>(), spawn_area);

    let alice = server
        .join_with(
            "Alice",
            BotOptions {
                view_distance: VIEW_DISTANCE,
            },
        )
        .await;

    // No explicit spawn height: the feet are on the custom surface of the
    // spawn column, asked of the custom generator.
    let (column_x, column_z) = SPAWN_COLUMN;
    let surface_y = surface(column_x, column_z);
    assert_eq!(surface_y, 106);
    assert_ne!(surface(37, -21), surface_y);
    assert_ne!(surface(38, -21), surface_y);
    assert_eq!(
        spawn_teleport(&alice).await,
        Vec3::new(SPAWN_X, f64::from(surface_y), SPAWN_Z)
    );
    assert_eq!(surface_calls(&calls), [SPAWN_COLUMN]);

    let in_view = square(spawn_chunk, i32::from(VIEW_DISTANCE));
    alice
        .wait_until("the chunks around the spawn", |view| {
            view.chunks.keys().copied().collect::<HashSet<_>>() == in_view
        })
        .await;
    let feet = BlockPos::new(column_x, surface_y, column_z);
    assert_eq!(alice.block_at(feet), Some(BlockState::AIR));
    assert_eq!(
        alice.block_at(feet.down(1)),
        Some(BlockState::from(BlockKind::OrangeWool)),
        "the player stands on the wool tile of the spawn column"
    );
    assert_eq!(
        alice.block_at(feet.down(2)),
        Some(BlockState::from(OakLog { axis: Axis::Z })),
    );
    assert_eq!(
        assert_pattern(&alice, "at spawn"),
        in_view.len(),
        "pre-generated and streamed chunks alike"
    );

    // Cross two chunk borders eastwards: new chunks are streamed and continue
    // the pattern; the westmost columns are forgotten.
    let start = alice.position();
    let mut position = start;
    for _ in 0..4 {
        position = Vec3::new(position.x + 8.0, position.y, position.z);
        alice.move_to(position).await;
    }
    let east = ChunkPos::new(4, -2);
    assert_eq!(ChunkPos::from(BlockPos::from(position)), east);
    let east_view = square(east, i32::from(VIEW_DISTANCE));
    alice
        .wait_until("the chunks two chunks east", |view| {
            view.chunk_center == Some(east)
                && view.chunks.keys().copied().collect::<HashSet<_>>() == east_view
        })
        .await;
    assert_pattern(&alice, "east of the spawn");

    // And back: the forgotten columns come back identical.
    while position.x > start.x {
        position = Vec3::new(position.x - 8.0, position.y, position.z);
        alice.move_to(position).await;
    }
    alice
        .wait_until("the chunks around the spawn again", |view| {
            view.chunk_center == Some(spawn_chunk)
                && view.chunks.keys().copied().collect::<HashSet<_>>() == in_view
        })
        .await;
    alice.sync().await;
    assert_eq!(loaded(&alice), in_view);
    assert_pattern(&alice, "back at the spawn");

    let packets = chunk_packets(&alice);
    let resent: Vec<(i32, i32)> = packets
        .iter()
        .filter(|(_, sent)| sent.len() > 1)
        .map(|(pos, _)| *pos)
        .collect();
    let forgotten: HashSet<ChunkPos> = in_view.difference(&east_view).copied().collect();
    assert_eq!(
        resent
            .iter()
            .map(|(x, z)| ChunkPos::new(*x, *z))
            .collect::<HashSet<_>>(),
        forgotten,
        "the chunks forgotten on the way east are sent again"
    );
    for pos in &resent {
        let sent = &packets[pos];
        assert!(
            sent.iter().all(|packet| *packet == sent[0]),
            "chunk {pos:?} changed between its first and its second send"
        );
    }

    // The server holds what the custom generator produced, and nothing else:
    // every chunk it indexes went through the custom generator.
    let generated: HashSet<ChunkPos> = generated(&calls).into_iter().collect();
    let explored: HashSet<ChunkPos> = in_view.union(&east_view).copied().collect();
    assert_eq!(indexed_chunks(&server).await, explored);
    assert_eq!(generated, explored);

    alice.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn an_explicit_spawn_height_wins_over_the_generator_surface() {
    let calls = CallLog::default();
    let generator_calls = calls.clone();
    let server = TestServer::builder()
        .config(move |config| {
            config
                .spawn_position(SpawnPosition {
                    x: SPAWN_X,
                    z: SPAWN_Z,
                    y: Some(150.0),
                })
                .world_generator(Tiles {
                    calls: generator_calls,
                })
        })
        .start()
        .await;
    let alice = server.join("Alice").await;

    assert_eq!(
        spawn_teleport(&alice).await,
        Vec3::new(SPAWN_X, 150.0, SPAWN_Z)
    );
    alice.sync().await;
    assert_eq!(alice.position(), Vec3::new(SPAWN_X, 150.0, SPAWN_Z));
    assert_eq!(
        surface_calls(&calls),
        [],
        "the generator's surface is not needed"
    );
    assert!(
        !generated(&calls).is_empty(),
        "the custom generator still shapes the world"
    );

    alice.assert_healthy();
    server.stop().await;
}

/// The example of `void-docs/docs/reference/gameplay/world.md`, "Custom World
/// Generators", copied verbatim except for the last statement, which becomes
/// `configure`: it must compile against the real API and work.
mod documented_example {
    use voidmc::{ChunkPos, ServerConfigBuilder, WorldGenerator};
    use voidmc_protocol::clientbound::chunk::{Chunk, ChunkBuilder, blocks};

    pub struct FlatWorldGenerator;

    impl WorldGenerator for FlatWorldGenerator {
        fn generate_chunk(&self, pos: &ChunkPos) -> Chunk {
            ChunkBuilder::new(pos.x, pos.z)
                .with_heightmap(|_, _| 64, blocks::GRASS_BLOCK)
                .build()
        }

        fn surface_height_at(&self, _block_x: i32, _block_z: i32) -> i32 {
            64
        }
    }

    pub fn configure(config: ServerConfigBuilder) -> ServerConfigBuilder {
        config.world_generator(FlatWorldGenerator)
    }
}

#[tokio::test]
async fn the_documented_flat_generator_works() {
    let server = TestServer::builder()
        .config(documented_example::configure)
        .start()
        .await;
    let alice = server.join("Alice").await;

    assert_eq!(spawn_teleport(&alice).await, Vec3::new(0.0, 64.0, 0.0));
    let corners = [BlockPos::new(0, 64, 0), BlockPos::new(-1, 64, -1)];
    alice
        .wait_until("the chunks on both sides of the origin", |view| {
            corners
                .iter()
                .all(|feet| view.chunks.contains_key(&ChunkPos::from(*feet)))
        })
        .await;
    for feet in corners {
        assert_eq!(alice.block_at(feet), Some(BlockState::AIR));
        assert_eq!(
            alice.block_at(feet.down(1)).map(BlockKind::from),
            Some(BlockKind::GrassBlock)
        );
    }

    alice.assert_healthy();
    server.stop().await;
}
