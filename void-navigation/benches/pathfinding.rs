use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use voidmc_navigation::pathing::{
    ArrayWorld, BlockPos, Cell, CellCache, FollowStatus, NavigationProfile, Path, PathFollower,
    Pathfinder, SearchRequest, Vec3,
};

const HALF: i32 = 96;
const HEIGHT: i32 = 48;

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }

    fn chance(&mut self, percent: u32) -> bool {
        self.next() % 100 < percent
    }
}

fn empty_world() -> ArrayWorld {
    ArrayWorld::new(BlockPos::new(-HALF, 0, -HALF), HALF * 2, HEIGHT, HALF * 2)
}

fn surface(x: i32, z: i32) -> i32 {
    let (x, z) = (x as f64, z as f64);
    let wave =
        (x * 0.09).sin() + (z * 0.07).sin() + (x * 0.23).sin() * 0.3 + (z * 0.19).sin() * 0.3;
    16 + (wave * 3.0) as i32
}

fn flat_world() -> ArrayWorld {
    let mut world = empty_world();
    world.fill(
        BlockPos::new(-HALF, 0, -HALF),
        BlockPos::new(HALF - 1, 15, HALF - 1),
        Cell::FULL,
    );
    world
}

fn hills_world() -> ArrayWorld {
    let mut world = empty_world();
    let mut rng = Lcg(7);
    for z in -HALF..HALF {
        for x in -HALF..HALF {
            let top = surface(x, z);
            world.fill(BlockPos::new(x, 0, z), BlockPos::new(x, top, z), Cell::FULL);
            if rng.chance(3) {
                world.fill(
                    BlockPos::new(x, top + 1, z),
                    BlockPos::new(x, top + 4, z),
                    Cell::FULL,
                );
            } else if rng.chance(2) {
                world.set(BlockPos::new(x, top + 1, z), Cell::solid(0, 8));
            }
        }
    }
    world
}

fn maze_world() -> ArrayWorld {
    let mut world = flat_world();
    let mut rng = Lcg(11);
    for x in (-HALF + 4..HALF - 4).step_by(4) {
        world.fill(
            BlockPos::new(x, 16, -HALF),
            BlockPos::new(x, 17, HALF - 1),
            Cell::FULL,
        );
        for _ in 0..3 {
            let gap = (rng.next() % (HALF as u32 * 2 - 8)) as i32 - HALF + 4;
            world.fill(
                BlockPos::new(x, 16, gap),
                BlockPos::new(x, 17, gap + 1),
                Cell::EMPTY,
            );
        }
    }
    world
}

fn sealed_world() -> ArrayWorld {
    let mut world = flat_world();
    world.fill(
        BlockPos::new(38, 16, -3),
        BlockPos::new(44, 19, 3),
        Cell::FULL,
    );
    world.fill(
        BlockPos::new(39, 16, -2),
        BlockPos::new(43, 18, 2),
        Cell::EMPTY,
    );
    world
}

fn feet_on(world: &ArrayWorld, x: i32, z: i32) -> Vec3 {
    let mut y = HEIGHT - 2;
    while y > 0 && !world.get(BlockPos::new(x, y - 1, z)).has_collision() {
        y -= 1;
    }
    while world.get(BlockPos::new(x, y, z)).has_collision() {
        y += 1;
    }
    BlockPos::new(x, y, z).bottom_center()
}

struct Scenario {
    name: &'static str,
    world: ArrayWorld,
    from: (i32, i32),
    to: (i32, i32),
    profile: NavigationProfile,
}

fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "flat_64",
            world: flat_world(),
            from: (-32, 0),
            to: (32, 0),
            profile: NavigationProfile::walker(),
        },
        Scenario {
            name: "hills_trees_96",
            world: hills_world(),
            from: (-48, -40),
            to: (48, 30),
            profile: NavigationProfile::walker(),
        },
        Scenario {
            name: "maze_160",
            world: maze_world(),
            from: (-82, 0),
            to: (82, 10),
            profile: NavigationProfile::walker()
                .search_limit(65_536)
                .nodes_per_block(0),
        },
        Scenario {
            name: "sealed_scaled_cap",
            world: sealed_world(),
            from: (0, 0),
            to: (41, 0),
            profile: NavigationProfile::walker().allow_partial(false),
        },
        Scenario {
            name: "sealed_full_cap_4096",
            world: sealed_world(),
            from: (0, 0),
            to: (41, 0),
            profile: NavigationProfile::walker()
                .allow_partial(false)
                .nodes_per_block(0),
        },
        Scenario {
            name: "hills_flyer_96",
            world: hills_world(),
            from: (-48, -40),
            to: (48, 30),
            profile: NavigationProfile::flyer(),
        },
    ]
}

fn report(scenario: &Scenario) {
    let mut cache = CellCache::new();
    let mut pathfinder = Pathfinder::new();
    let mut path = Path::default();
    let request = SearchRequest::new(
        feet_on(&scenario.world, scenario.from.0, scenario.from.1),
        feet_on(&scenario.world, scenario.to.0, scenario.to.1),
    );
    let status = pathfinder.find(
        &mut cache.view(&scenario.world),
        &scenario.profile,
        request,
        &mut path,
    );
    eprintln!(
        "{}: {:?}, expanded {}, visited {}, waypoints {}, sections {}",
        scenario.name,
        status,
        pathfinder.stats().expanded,
        pathfinder.stats().visited,
        path.len(),
        cache.stats().sections,
    );
}

fn search_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("search");
    for scenario in scenarios() {
        report(&scenario);
        let request = SearchRequest::new(
            feet_on(&scenario.world, scenario.from.0, scenario.from.1),
            feet_on(&scenario.world, scenario.to.0, scenario.to.1),
        );
        let mut cache = CellCache::new();
        let mut pathfinder = Pathfinder::new();
        let mut path = Path::default();
        group.bench_function(BenchmarkId::new("warm_cache", scenario.name), |b| {
            b.iter(|| {
                let status = pathfinder.find(
                    &mut cache.view(&scenario.world),
                    &scenario.profile,
                    black_box(request),
                    &mut path,
                );
                black_box(status)
            })
        });
        group.bench_function(BenchmarkId::new("cold_cache", scenario.name), |b| {
            b.iter(|| {
                let mut cold = CellCache::new();
                let status = pathfinder.find(
                    &mut cold.view(&scenario.world),
                    &scenario.profile,
                    black_box(request),
                    &mut path,
                );
                black_box(status)
            })
        });
    }
    group.finish();
}

fn budgeted_benches(c: &mut Criterion) {
    let world = hills_world();
    let mut group = c.benchmark_group("budgeted");
    let budget = 2_000u32;
    group.throughput(Throughput::Elements(budget as u64));
    let mut cache = CellCache::new();
    let mut pathfinder = Pathfinder::new();
    let model = NavigationProfile::walker().search_limit(1 << 20).compile();
    let request = SearchRequest::new(feet_on(&world, -90, -90), feet_on(&world, 90, 90));
    group.bench_function("start_then_expand_2000_nodes", |b| {
        b.iter(|| {
            let mut view = cache.view(&world);
            pathfinder.start(&mut view, &model, request);
            let mut remaining = budget;
            let status = pathfinder.step(&mut view, &mut remaining);
            black_box((status, remaining))
        })
    });
    group.finish();
}

fn follow_benches(c: &mut Criterion) {
    let world = hills_world();
    let profile = NavigationProfile::walker();
    let model = profile.compile();
    let mut cache = CellCache::new();
    let mut pathfinder = Pathfinder::new();
    let mut paths = Vec::new();
    let mut rng = Lcg(3);
    for _ in 0..500 {
        let (x, z) = (
            (rng.next() % 120) as i32 - 60,
            (rng.next() % 120) as i32 - 60,
        );
        let mut path = Path::default();
        pathfinder.find(
            &mut cache.view(&world),
            &profile,
            SearchRequest::new(feet_on(&world, x, z), feet_on(&world, x + 20, z + 10)),
            &mut path,
        );
        let start = feet_on(&world, x, z);
        paths.push((start, start, path, PathFollower::default()));
    }
    let mut group = c.benchmark_group("follow");
    group.throughput(Throughput::Elements(paths.len() as u64));
    group.bench_function("500_followers_one_tick", |b| {
        b.iter(|| {
            let mut moving = 0u32;
            for (start, position, path, follower) in paths.iter_mut() {
                if let FollowStatus::Moving(steering) =
                    follower.tick(path, *position, true, 0.1, &model)
                {
                    *position += steering.velocity;
                    moving += 1;
                } else {
                    *position = *start;
                    follower.reset();
                }
            }
            black_box(moving)
        })
    });
    group.finish();
}

criterion_group!(benches, search_benches, budgeted_benches, follow_benches);
criterion_main!(benches);
