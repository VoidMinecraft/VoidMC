//! VMC-ATP-028: independent regions and systems are processed concurrently
//! without corrupting the world state.
//!
//! Two independent regions near spawn, region (0, 0) and region (-1, -1), each
//! get a scheduled builder system that places one block per tick in its own
//! chunk, and pigs that fall onto (or past) what is built. The same scenario
//! runs on the single-threaded baseline and, several times, with
//! `ExecutionMode::MultiThreaded`; every run must end in the same world state.
//! Under multi-threading the two builders must meet inside their bodies on
//! every tick, which only happens if they really run at the same time.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use bevy_app::{App, Last, Update};
use bevy_ecs::prelude::*;
use voidmc::ExecutionMode;
use voidmc::components::{Grounded, Position, Velocity, VerticalVelocity};
use voidmc::entity::{EntityBuilder, EntityKind};
use voidmc::schedule::VoidSystems;
use voidmc::world::{ChunkData, ChunkIndex, ChunkPos, DimensionId, RegionPos};
use voidmc_e2e::TestServer;
use voidmc_e2e::core::position::BlockPos;
use voidmc_e2e::registry::builtin::BlockKind;
use voidmc_protocol::clientbound::chunk::blocks;

/// Ticks each scenario run lasts.
const TICKS: u32 = 100;
/// Multi-threaded repetitions compared against the one baseline run.
const PARALLEL_RUNS: usize = 3;
/// Height of the platform the builders lay down, one block per tick.
const PLATFORM_Y: i32 = 120;
/// How long a builder waits inside its body for the other one.
const MEET_BOUND: Duration = Duration::from_secs(10);
const SERVER_THREAD: &str = "void-e2e-server";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    A,
    B,
}

impl Side {
    /// One chunk per region: chunk (1, 1) is in region (0, 0), chunk
    /// (-2, -2) in region (-1, -1).
    fn chunk(self) -> ChunkPos {
        match self {
            Side::A => ChunkPos::new(1, 1),
            Side::B => ChunkPos::new(-2, -2),
        }
    }

    fn region(self) -> RegionPos {
        RegionPos::of_chunk(DimensionId::Overworld, self.chunk())
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// The block the builders place on their `k`-th tick, and where.
fn build_step(k: u32) -> (u8, i32, u8, i32) {
    let block = [blocks::STONE, blocks::GRASS_BLOCK, blocks::DIRT][k as usize % 3];
    ((k % 16) as u8, PLATFORM_Y, (k / 16) as u8, block)
}

fn kind_of(block: i32) -> BlockKind {
    match block {
        blocks::STONE => BlockKind::Stone,
        blocks::GRASS_BLOCK => BlockKind::GrassBlock,
        blocks::DIRT => BlockKind::Dirt,
        other => panic!("unexpected block {other}"),
    }
}

#[derive(Component)]
struct InRegionA;

#[derive(Component)]
struct InRegionB;

/// Orders the pigs between runs; their network ids differ from run to run.
#[derive(Component, Clone, Copy)]
struct ScenarioId(u32);

/// Everything compared between runs, floats as raw bits.
#[derive(Clone, Debug, PartialEq, Eq)]
struct WorldState {
    /// Per pig: position, velocity, vertical velocity, grounded.
    pigs: Vec<(u32, [u64; 7], bool)>,
    /// Encoded block sections of each region's chunk.
    chunks: [Vec<u8>; 2],
    /// The build steps each builder applied, in order.
    applied: [Vec<u32>; 2],
}

/// Generation-counted meeting point for the two builders.
#[derive(Default)]
struct Meeting {
    state: Mutex<(u64, usize)>,
    all_in: Condvar,
}

impl Meeting {
    /// Whether the other builder arrived within the bound.
    fn meet(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        let generation = state.0;
        state.1 += 1;
        if state.1 == 2 {
            *state = (generation + 1, 0);
            self.all_in.notify_all();
            return true;
        }
        let (mut state, _) = self
            .all_in
            .wait_timeout_while(state, MEET_BOUND, |state| state.0 == generation)
            .unwrap();
        if state.0 != generation {
            return true;
        }
        state.1 -= 1;
        false
    }
}

#[derive(Default)]
struct Record {
    /// Steps left per builder.
    budget: [u32; 2],
    applied: [Vec<u32>; 2],
    in_flight: usize,
    most_in_flight: usize,
    /// Ticks on which the builders met inside their bodies.
    meetings: [u32; 2],
    threads: Vec<String>,
    snapshot: Option<WorldState>,
}

/// Shared between the test and the server's systems.
#[derive(Resource, Clone)]
struct Probe {
    mode: ExecutionMode,
    record: Arc<Mutex<Record>>,
    meeting: Arc<Meeting>,
    gave_up: Arc<AtomicBool>,
}

impl Probe {
    fn new(mode: ExecutionMode) -> Self {
        Self {
            mode,
            record: Arc::default(),
            meeting: Arc::default(),
            gave_up: Arc::default(),
        }
    }

    /// Runs one builder step for `side`, if it has budget left.
    fn build(&self, side: Side, chunk: &mut ChunkData) {
        let k = {
            let mut record = self.record.lock().unwrap();
            if record.budget[side.index()] == 0 {
                return;
            }
            record.budget[side.index()] -= 1;
            record.in_flight += 1;
            record.most_in_flight = record.most_in_flight.max(record.in_flight);
            let thread = std::thread::current();
            record
                .threads
                .push(thread.name().unwrap_or("<unnamed>").to_string());
            record.applied[side.index()].len() as u32
        };

        if self.mode == ExecutionMode::MultiThreaded && !self.gave_up.load(Ordering::SeqCst) {
            if self.meeting.meet() {
                self.record.lock().unwrap().meetings[side.index()] += 1;
            } else {
                self.gave_up.store(true, Ordering::SeqCst);
            }
        }

        let (x, y, z, block) = build_step(k);
        chunk.set_block(x, y, z, block);

        let mut record = self.record.lock().unwrap();
        record.applied[side.index()].push(k);
        record.in_flight -= 1;
    }
}

fn build_region_a(
    probe: Res<Probe>,
    mut chunk: Query<&mut ChunkData, (With<InRegionA>, Without<InRegionB>)>,
) {
    if let Ok(mut chunk) = chunk.single_mut() {
        probe.build(Side::A, &mut chunk);
    }
}

fn build_region_b(
    probe: Res<Probe>,
    mut chunk: Query<&mut ChunkData, (With<InRegionB>, Without<InRegionA>)>,
) {
    if let Ok(mut chunk) = chunk.single_mut() {
        probe.build(Side::B, &mut chunk);
    }
}

/// Snapshots the world at the end of the tick on which both builders used
/// their last step, so every run is compared after exactly `TICKS` ticks.
fn capture(world: &mut World) {
    let probe = world.resource::<Probe>().clone();
    let done = {
        let record = probe.record.lock().unwrap();
        record.snapshot.is_none()
            && record
                .applied
                .iter()
                .all(|applied| applied.len() == TICKS as usize)
    };
    if !done {
        return;
    }

    let mut pigs: Vec<(u32, [u64; 7], bool)> = world
        .query::<(
            &ScenarioId,
            &Position,
            &Velocity,
            &VerticalVelocity,
            &Grounded,
        )>()
        .iter(world)
        .map(|(id, p, v, vertical, grounded)| {
            let bits = [p.x, p.y, p.z, v.x, v.y, v.z, vertical.0].map(f64::to_bits);
            (id.0, bits, grounded.0)
        })
        .collect();
    pigs.sort_by_key(|(id, ..)| *id);

    let chunks = [Side::A, Side::B].map(|side| {
        let entity = world.resource::<ChunkIndex>().0[&(DimensionId::Overworld, side.chunk())];
        let chunk = world.get::<ChunkData>(entity).expect("region chunk data");
        chunk
            .sections
            .iter()
            .flat_map(|section| section.encode_to_bytes())
            .collect::<Vec<u8>>()
    });

    let mut record = probe.record.lock().unwrap();
    let applied = record.applied.clone();
    record.snapshot = Some(WorldState {
        pigs,
        chunks,
        applied,
    });
}

/// Pigs above each region's platform, and one flyer per region whose
/// position counts the physics steps it took.
fn spawn_pigs(world: &mut World) -> Vec<(u32, Position, Velocity)> {
    let mut flyers = Vec::new();
    let mut id = 0;
    for side in [Side::A, Side::B] {
        let origin_x = (side.chunk().x * 16) as f64;
        let origin_z = (side.chunk().z * 16) as f64;
        for i in 0..8 {
            let velocity = Velocity {
                x: 0.02 * (i % 3) as f64,
                y: 0.0,
                z: 0.05,
            };
            EntityBuilder::new(EntityKind::Pig)
                .at(
                    origin_x + 1.5 + 2.0 * i as f64,
                    (PLATFORM_Y + 10) as f64,
                    origin_z + 0.5 + (i % 4) as f64,
                )
                .velocity(velocity)
                .gravity(true)
                .block_collision(true)
                .wander(false)
                .with(ScenarioId(id))
                .spawn_in(world);
            id += 1;
        }

        let start = Position {
            x: origin_x + 8.0,
            y: 200.0,
            z: origin_z + 8.0,
        };
        let velocity = Velocity {
            x: 0.1,
            y: 0.0,
            z: -0.05,
        };
        EntityBuilder::new(EntityKind::Pig)
            .position(start)
            .velocity(velocity)
            .gravity(false)
            .block_collision(false)
            .wander(false)
            .with(ScenarioId(id))
            .spawn_in(world);
        flyers.push((id, start, velocity));
        id += 1;
    }
    flyers
}

struct Run {
    state: WorldState,
    record_threads: Vec<String>,
    most_in_flight: usize,
    meetings: [u32; 2],
    gave_up: bool,
}

async fn run_scenario(mode: ExecutionMode) -> Run {
    let probe = Probe::new(mode);
    let plugin_probe = probe.clone();
    let server = TestServer::builder()
        .config(move |config| config.execution_mode(mode))
        .plugin(move |app: &mut App| {
            app.insert_resource(plugin_probe)
                .add_systems(
                    Update,
                    (build_region_a, build_region_b).before(VoidSystems::EntitySimulation),
                )
                .add_systems(Last, capture);
        })
        .start()
        .await;

    assert_eq!(
        server
            .with_world(|world| *world.resource::<ExecutionMode>())
            .await,
        mode,
        "the server runs in the configured execution mode"
    );
    assert_ne!(
        Side::A.region(),
        Side::B.region(),
        "two independent regions"
    );

    // Mark each region's chunk, spawn the pigs and arm both builders in one
    // closure, so builders and physics start on the same tick.
    let arm = probe.record.clone();
    let flyers = server
        .with_world(move |world| {
            for side in [Side::A, Side::B] {
                let entity = *world
                    .resource::<ChunkIndex>()
                    .0
                    .get(&(DimensionId::Overworld, side.chunk()))
                    .expect("the region's chunk is in the spawn area");
                match side {
                    Side::A => world.entity_mut(entity).insert(InRegionA),
                    Side::B => world.entity_mut(entity).insert(InRegionB),
                };
            }
            let flyers = spawn_pigs(world);
            arm.lock().unwrap().budget = [TICKS; 2];
            flyers
        })
        .await;

    // Liveness: every tick of the scenario completes within the bounded wait.
    let record = probe.record.clone();
    let state = server
        .wait_for(
            "both regions to run their scheduled systems for every tick",
            move |_| record.lock().unwrap().snapshot.clone(),
        )
        .await;

    // No lost or duplicate physics step: a flyer moves by its velocity once
    // per tick, so its position counts the steps it took.
    for (id, start, velocity) in flyers {
        let (_, bits, _) = state.pigs.iter().find(|(pig, ..)| *pig == id).unwrap();
        let mut expected = start;
        for _ in 0..TICKS {
            expected.x += velocity.x;
            expected.z += velocity.z;
        }
        assert_eq!(
            [bits[0], bits[1], bits[2]],
            [expected.x, expected.y, expected.z].map(f64::to_bits),
            "{mode:?}: flyer {id} took a different number of physics steps than {TICKS}"
        );
    }

    // A client sees both regions' builds.
    let watcher = server.join("Watcher").await;
    for side in [Side::A, Side::B] {
        let origin = (side.chunk().x * 16, side.chunk().z * 16);
        let expected: Vec<(BlockPos, BlockKind)> = (0..TICKS)
            .map(|k| {
                let (x, y, z, block) = build_step(k);
                (
                    BlockPos::new(origin.0 + x as i32, y, origin.1 + z as i32),
                    kind_of(block),
                )
            })
            .collect();
        watcher
            .wait_until("the client to hold both regions' builds", move |view| {
                expected
                    .iter()
                    .all(|(pos, kind)| view.block_at(*pos).map(BlockKind::from) == Some(*kind))
            })
            .await;
    }
    watcher.assert_healthy();

    // The server keeps ticking after the scenario and stops cleanly.
    server.wait_ticks(5).await;
    server.stop().await;

    let record = probe.record.lock().unwrap();
    Run {
        state,
        record_threads: record.threads.clone(),
        most_in_flight: record.most_in_flight,
        meetings: record.meetings,
        gave_up: probe.gave_up.load(Ordering::SeqCst),
    }
}

#[tokio::test]
async fn regions_and_systems_run_in_parallel_and_match_the_single_thread_baseline() {
    let baseline = run_scenario(ExecutionMode::SingleThreaded).await;

    // Every scheduled update applied exactly once, in order.
    let all_steps: Vec<u32> = (0..TICKS).collect();
    assert_eq!(
        baseline.state.applied,
        [all_steps.clone(), all_steps.clone()]
    );
    // The baseline really is serial, on the game thread.
    assert_eq!(baseline.most_in_flight, 1, "baseline builders overlapped");
    assert!(
        baseline
            .record_threads
            .iter()
            .all(|name| name == SERVER_THREAD),
        "baseline systems left the game thread: {:?}",
        baseline.record_threads
    );
    assert_eq!(baseline.meetings, [0, 0]);
    // The scenario is not trivial: pigs landed on the platform the builders
    // laid, and others fell past where it was not built yet.
    let y = |bits: &[u64; 7]| f64::from_bits(bits[1]);
    assert!(
        baseline
            .state
            .pigs
            .iter()
            .any(|(_, bits, grounded)| *grounded && y(bits) == (PLATFORM_Y + 1) as f64),
        "no pig landed on a built block: {:?}",
        baseline.state.pigs
    );
    assert!(
        baseline
            .state
            .pigs
            .iter()
            .any(|(_, bits, _)| y(bits) < PLATFORM_Y as f64),
        "no pig fell past the platform: {:?}",
        baseline.state.pigs
    );

    for run in 0..PARALLEL_RUNS {
        let parallel = run_scenario(ExecutionMode::MultiThreaded).await;

        // Independent systems ran concurrently: the builders met inside
        // their bodies on every tick, on the compute pool, not the game thread.
        assert!(
            !parallel.gave_up,
            "run {run}: the two region systems were never in flight at the same time"
        );
        assert_eq!(parallel.meetings, [TICKS, TICKS], "run {run}");
        assert_eq!(parallel.most_in_flight, 2, "run {run}");
        assert!(
            parallel
                .record_threads
                .iter()
                .all(|name| name.starts_with("Compute Task Pool")),
            "run {run}: systems ran outside the compute pool: {:?}",
            parallel.record_threads
        );

        // Same world state as the baseline, every update applied once.
        assert_eq!(
            parallel.state.applied, baseline.state.applied,
            "run {run}: a scheduled update was lost or applied twice"
        );
        assert_eq!(
            parallel.state.chunks, baseline.state.chunks,
            "run {run}: the regions' blocks differ from the baseline"
        );
        for ((id, bits, grounded), (_, base_bits, base_grounded)) in
            parallel.state.pigs.iter().zip(&baseline.state.pigs)
        {
            assert_eq!(
                (bits, grounded),
                (base_bits, base_grounded),
                "run {run}: pig {id} differs from the baseline"
            );
        }
        assert_eq!(parallel.state, baseline.state, "run {run}");
    }
}
