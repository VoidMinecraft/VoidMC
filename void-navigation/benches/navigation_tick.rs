use std::hint::black_box;
use std::time::{Duration, Instant};

use bevy_app::{App, Update};
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemState;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use voidmc::events::BlockChangeEvent;
use voidmc::systems::physics::apply_spawned_entity_physics;
use voidmc::world::generation::{generate_chunk, surface_height_at};
use voidmc::world::{ChunkData, ChunkDimension, ChunkIndex, ChunkPos, ChunkPosition, DimensionId};
use voidmc::{EntityBuilder, EntityKind};
use voidmc_data::v26_1_2::blocks;
use voidmc_navigation::pathing::{BlockPos, NavWorld};
use voidmc_navigation::{
    Behaviour, Behaviours, ChunkCells, Goal, NavigationPlugin, NavigationProfile, NavigationStats,
    NavigationSystems, NavigationWorld, Navigator,
};
use voidmc_protocol::types::BlockPosition;

const RADIUS_CHUNKS: i32 = 6;
const WARMUP_TICKS: usize = 100;

#[derive(Clone, Copy)]
enum Load {
    Wander,
    Patrol,
}

fn world(navigation: bool) -> App {
    let mut app = App::new();
    app.init_resource::<ChunkIndex>();
    if navigation {
        app.add_plugins(NavigationPlugin::new());
        app.add_systems(
            Update,
            apply_spawned_entity_physics.after(NavigationSystems),
        );
    } else {
        app.add_systems(Update, apply_spawned_entity_physics);
    }
    for x in -RADIUS_CHUNKS..RADIUS_CHUNKS {
        for z in -RADIUS_CHUNKS..RADIUS_CHUNKS {
            let pos = ChunkPos::new(x, z);
            let data = ChunkData::from_protocol_chunk(&generate_chunk(&pos));
            let entity = app
                .world_mut()
                .spawn((
                    ChunkPosition(pos),
                    data,
                    ChunkDimension(DimensionId::Overworld),
                ))
                .id();
            app.world_mut()
                .resource_mut::<ChunkIndex>()
                .0
                .insert((DimensionId::Overworld, pos), entity);
        }
    }
    app
}

fn populate(app: &mut App, mobs: usize, load: Load, navigation: bool) {
    let span = (RADIUS_CHUNKS * 16 - 24) as f64;
    for i in 0..mobs {
        let angle = i as f64 * 2.399_963;
        let reach = span * ((i as f64 + 0.5) / mobs as f64).sqrt();
        let (x, z) = (angle.cos() * reach, angle.sin() * reach);
        let y = surface_height_at(x.floor() as i32, z.floor() as i32) as f64;
        let mut builder = EntityBuilder::new(EntityKind::Zombie)
            .at(x.floor() + 0.5, y, z.floor() + 0.5)
            .gravity(true)
            .block_collision(true)
            .settle_ticks(0);
        if navigation {
            let navigator =
                Navigator::new(NavigationProfile::walker().size(0.6, 1.95).step_height(1.0))
                    .with_speed(0.2);
            builder = match load {
                Load::Wander => builder
                    .with(navigator)
                    .with(Behaviours::new().with(Behaviour::wander([x, y, z], 16.0))),
                Load::Patrol => builder.with(navigator.with_goal(Goal::patrol([
                    [x, y, z],
                    [x + 20.0 * angle.sin(), y, z - 20.0 * angle.cos()],
                ]))),
            };
        }
        builder.spawn_in(app.world_mut());
    }
    for _ in 0..WARMUP_TICKS {
        app.update();
    }
}

fn tick_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("navigation_tick");
    group.measurement_time(Duration::from_secs(5));
    for (load, name) in [(Load::Wander, "wander"), (Load::Patrol, "patrol")] {
        for mobs in [100usize, 500, 1000] {
            group.throughput(Throughput::Elements(mobs as u64));
            let mut app = world(true);
            populate(&mut app, mobs, load, true);
            let initial = *app.world().resource::<NavigationStats>();
            let mut searches = initial.searches;
            let started = Instant::now();
            let mut ticks = 0u64;
            let mut planning = 0u64;
            let mut peak = 0u64;
            group.bench_function(BenchmarkId::new(name, mobs), |b| {
                b.iter(|| {
                    app.update();
                    let stats = app.world().resource::<NavigationStats>();
                    ticks += 1;
                    planning += stats.planning_micros_last_tick;
                    peak = peak.max(stats.planning_micros_last_tick);
                    black_box(stats.moving)
                })
            });
            let stats = *app.world().resource::<NavigationStats>();
            searches = stats.searches - searches;
            let seconds = started.elapsed().as_secs_f64();
            let expanded = stats.expanded - initial.expanded;
            eprintln!(
                "{name}/{mobs}: {ticks} ticks, {searches} searches ({:.0}/s wall, {:.2}/tick, {:.0} nodes each, {:.0} ns/node), planning avg {:.1} µs peak {peak} µs, moving {}, queued {}",
                searches as f64 / seconds,
                searches as f64 / ticks.max(1) as f64,
                expanded as f64 / searches.max(1) as f64,
                planning as f64 * 1000.0 / expanded.max(1) as f64,
                planning as f64 / ticks.max(1) as f64,
                stats.moving,
                stats.queued,
            );
        }
    }
    for mobs in [100usize, 500, 1000] {
        let mut app = world(false);
        populate(&mut app, mobs, Load::Wander, false);
        group.throughput(Throughput::Elements(mobs as u64));
        group.bench_function(BenchmarkId::new("physics_only_baseline", mobs), |b| {
            b.iter(|| app.update())
        });
    }
    let mut app = world(true);
    populate(&mut app, 500, Load::Patrol, true);
    let initial = *app.world().resource::<NavigationStats>();
    let mut toggle = false;
    group.throughput(Throughput::Elements(500));
    group.bench_function(BenchmarkId::new("patrol_while_digging", 500), |b| {
        b.iter(|| {
            toggle = !toggle;
            dig(&mut app, toggle);
            app.update();
            black_box(app.world().resource::<NavigationStats>().moving)
        })
    });
    let stats = *app.world().resource::<NavigationStats>();
    eprintln!(
        "patrol_while_digging/500: {} searches, {} cache hits",
        stats.searches - initial.searches,
        stats.cache_hits - initial.cache_hits,
    );
    group.finish();
}

fn dig(app: &mut App, solid: bool) {
    let (x, z) = (RADIUS_CHUNKS * 16 - 4, RADIUS_CHUNKS * 16 - 4);
    let y = surface_height_at(x, z);
    let state = if solid { blocks::STONE } else { blocks::AIR };
    let pos = ChunkPos::new(x >> 4, z >> 4);
    let entity = app.world().resource::<ChunkIndex>().0[&(DimensionId::Overworld, pos)];
    let old = app
        .world_mut()
        .get_mut::<ChunkData>(entity)
        .expect("chunk")
        .set_block((x & 15) as u8, y, (z & 15) as u8, state)
        .unwrap_or(0);
    app.world_mut().trigger(BlockChangeEvent {
        dimension: DimensionId::Overworld,
        position: BlockPosition { x, y: y as i16, z },
        old_state: old,
        new_state: state,
        source: None,
    });
}

fn fill_benches(c: &mut Criterion) {
    let mut app = world(true);
    let mut state: SystemState<(ResMut<NavigationWorld>, ChunkCells)> =
        SystemState::new(app.world_mut());
    let (mut navigation, chunks) = state.get_mut(app.world_mut());
    let mut group = c.benchmark_group("cell_cache");
    let mut chunk = 0;
    group.bench_function("fill_terrain_section", |b| {
        b.iter(|| {
            chunk = (chunk + 1) % 64;
            let pos = ChunkPos::new(chunk % 8 - 4, chunk / 8 - 4);
            navigation.invalidate_chunk(DimensionId::Overworld, pos);
            let cell = navigation
                .view(DimensionId::Overworld, &chunks)
                .cell(BlockPos::new(pos.x * 16, 60, pos.z * 16));
            black_box(cell)
        })
    });
    group.finish();
}

criterion_group!(benches, tick_benches, fill_benches);
criterion_main!(benches);
