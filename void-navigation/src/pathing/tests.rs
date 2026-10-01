use super::*;

const GROUND: i32 = 0;

fn flat() -> ArrayWorld {
    let mut world = ArrayWorld::new(BlockPos::new(-32, -4, -32), 64, 32, 64);
    world.fill(
        BlockPos::new(-32, -4, -32),
        BlockPos::new(31, GROUND, 31),
        Cell::FULL,
    );
    world
}

fn feet(x: i32, z: i32) -> Vec3 {
    BlockPos::new(x, GROUND + 1, z).bottom_center()
}

fn search(
    world: &mut ArrayWorld,
    profile: &NavigationProfile,
    from: Vec3,
    to: Vec3,
) -> (SearchStatus, Path) {
    let mut pathfinder = Pathfinder::new();
    let mut path = Path::default();
    let status = pathfinder.find(world, profile, SearchRequest::new(from, to), &mut path);
    (status, path)
}

fn walker() -> NavigationProfile {
    NavigationProfile::walker()
}

fn wall(world: &mut ArrayWorld, x: i32, z0: i32, z1: i32, height: i32) {
    world.fill(
        BlockPos::new(x, GROUND + 1, z0),
        BlockPos::new(x, GROUND + height, z1),
        Cell::FULL,
    );
}

#[test]
fn straight_line_on_open_ground_collapses_to_few_waypoints() {
    let mut world = flat();
    let (status, path) = search(&mut world, &walker(), feet(0, 0), feet(12, 0));
    assert_eq!(status, SearchStatus::Complete);
    assert!(path.is_complete());
    assert_eq!(path.points(), &[feet(12, 0)]);

    let (_, long) = search(&mut world, &walker(), feet(-25, 0), feet(25, 0));
    assert!(long.len() <= 4, "{:?}", long.points());
    assert!(long.points().iter().all(|p| p.z == 0.5 && p.y == 1.0));
}

#[test]
fn detours_around_a_wall() {
    let mut world = flat();
    wall(&mut world, 5, -6, 6, 3);
    let (status, path) = search(&mut world, &walker(), feet(0, 0), feet(10, 0));
    assert_eq!(status, SearchStatus::Complete);
    assert_eq!(path.end(), Some(feet(10, 0)));
    assert!(path.len() >= 2);
    assert!(
        path.points()
            .iter()
            .all(|p| BlockPos::containing(*p).x != 5 || p.z.abs() > 6.0)
    );
}

#[test]
fn enclosed_goal_is_unreachable_without_partial_paths() {
    let mut world = flat();
    world.fill(BlockPos::new(8, 1, -2), BlockPos::new(12, 3, 2), Cell::FULL);
    world.fill(
        BlockPos::new(9, 1, -1),
        BlockPos::new(11, 3, 1),
        Cell::EMPTY,
    );
    let profile = walker().allow_partial(false);
    let (status, path) = search(&mut world, &profile, feet(0, 0), feet(10, 0));
    assert_eq!(status, SearchStatus::Unreachable);
    assert!(path.is_empty());
}

#[test]
fn enclosed_goal_yields_the_closest_partial_path() {
    let mut world = flat();
    world.fill(BlockPos::new(8, 1, -2), BlockPos::new(12, 3, 2), Cell::FULL);
    world.fill(
        BlockPos::new(9, 1, -1),
        BlockPos::new(11, 3, 1),
        Cell::EMPTY,
    );
    let (status, path) = search(&mut world, &walker(), feet(0, 0), feet(10, 0));
    assert_eq!(status, SearchStatus::Partial);
    assert!(!path.is_complete());
    let end = path.end().expect("partial path");
    assert!(end.horizontal_distance(feet(10, 0)) <= 3.0, "{end:?}");
}

#[test]
fn steps_up_one_block_but_not_two() {
    let mut world = flat();
    world.fill(
        BlockPos::new(3, 1, -20),
        BlockPos::new(20, 1, 20),
        Cell::FULL,
    );
    let (status, path) = search(
        &mut world,
        &walker(),
        feet(0, 0),
        BlockPos::new(6, 2, 0).bottom_center(),
    );
    assert_eq!(status, SearchStatus::Complete);
    assert_eq!(path.end().map(|p| p.y), Some(2.0));

    let mut cliff = flat();
    cliff.fill(
        BlockPos::new(3, 1, -32),
        BlockPos::new(31, 2, 31),
        Cell::FULL,
    );
    let profile = walker().allow_partial(false);
    let (status, _) = search(
        &mut cliff,
        &profile,
        feet(0, 0),
        BlockPos::new(6, 3, 0).bottom_center(),
    );
    assert_eq!(status, SearchStatus::Unreachable);
}

#[test]
fn jump_rises_cost_more_than_steps() {
    let model = walker().compile();
    let mut world = flat();
    world.set(BlockPos::new(1, 1, 0), Cell::FULL);
    world.set(BlockPos::new(-1, 1, 0), Cell::solid(0, 8));
    let mut moves = Vec::new();
    neighbors(&model, &mut world, BlockPos::new(0, 1, 0), 16, &mut moves);
    let jump = moves
        .iter()
        .find(|m| m.pos == BlockPos::new(1, 2, 0))
        .expect("jump");
    let slab = moves
        .iter()
        .find(|m| m.pos == BlockPos::new(-1, 1, 0))
        .expect("slab");
    let flat = moves
        .iter()
        .find(|m| m.pos == BlockPos::new(0, 1, 1))
        .expect("flat");
    assert_eq!(slab.floor, 24);
    assert_eq!(slab.cost, flat.cost);
    assert!(jump.cost > flat.cost);
}

#[test]
fn drops_within_the_fall_limit_only() {
    let mut world = ArrayWorld::new(BlockPos::new(-8, -8, -8), 32, 32, 16);
    world.fill(
        BlockPos::new(-8, -8, -8),
        BlockPos::new(-1, 4, 7),
        Cell::FULL,
    );
    world.fill(
        BlockPos::new(0, -8, -8),
        BlockPos::new(23, 1, 7),
        Cell::FULL,
    );
    let start = BlockPos::new(-2, 5, 0).bottom_center();
    let low = BlockPos::new(4, 2, 0).bottom_center();
    let profile = walker().allow_partial(false);
    let (status, _) = search(&mut world, &profile, start, low);
    assert_eq!(status, SearchStatus::Complete);
    let (status, _) = search(&mut world, &profile.clone().max_fall(2), start, low);
    assert_eq!(status, SearchStatus::Unreachable);
}

#[test]
fn walks_on_slabs_without_jumping() {
    let mut world = flat();
    world.fill(
        BlockPos::new(2, 1, -1),
        BlockPos::new(8, 1, 1),
        Cell::solid(0, 8),
    );
    world.fill(
        BlockPos::new(-31, 1, 2),
        BlockPos::new(31, 3, 31),
        Cell::FULL,
    );
    world.fill(
        BlockPos::new(-31, 1, -32),
        BlockPos::new(31, 3, -2),
        Cell::FULL,
    );
    let profile = walker().jump_height(0.0);
    let (status, path) = search(
        &mut world,
        &profile,
        feet(0, 0),
        BlockPos::new(5, 1, 0).bottom_center(),
    );
    assert_eq!(status, SearchStatus::Complete);
    assert_eq!(path.end().map(|p| p.y), Some(1.5));
}

#[test]
fn fences_cannot_be_jumped() {
    let mut world = flat();
    world.fill(
        BlockPos::new(5, 1, -32),
        BlockPos::new(5, 1, 31),
        Cell::solid(6, 24),
    );
    let profile = walker().allow_partial(false);
    let (status, _) = search(&mut world, &profile, feet(0, 0), feet(10, 0));
    assert_eq!(status, SearchStatus::Unreachable);
}

#[test]
fn closed_doors_block_and_open_doors_pass() {
    let mut world = flat();
    wall(&mut world, 5, -32, 31, 3);
    let door = BlockPos::new(5, 1, 0);
    let closed = Cell::EMPTY.with_edges(Side::West.mask());
    world.set(door, closed);
    world.set(door.offset(0, 1, 0), closed);
    let profile = walker().allow_partial(false);
    let (status, _) = search(&mut world, &profile, feet(0, 0), feet(10, 0));
    assert_eq!(status, SearchStatus::Unreachable);

    let open = Cell::EMPTY.with_edges(Side::North.mask());
    world.set(door, open);
    world.set(door.offset(0, 1, 0), open);
    let (status, path) = search(&mut world, &profile, feet(0, 0), feet(10, 0));
    assert_eq!(status, SearchStatus::Complete);
    assert!(
        path.points()
            .iter()
            .any(|p| BlockPos::containing(*p) == door || BlockPos::containing(*p).x > 5)
    );
}

#[test]
fn low_ceilings_stop_tall_bodies_but_not_short_ones() {
    let mut world = flat();
    world.fill(
        BlockPos::new(-31, 1, -32),
        BlockPos::new(31, 2, -1),
        Cell::FULL,
    );
    world.fill(
        BlockPos::new(-31, 1, 1),
        BlockPos::new(31, 2, 31),
        Cell::FULL,
    );
    world.fill(BlockPos::new(3, 2, 0), BlockPos::new(7, 2, 0), Cell::FULL);
    let tall = walker().allow_partial(false);
    let (status, _) = search(&mut world, &tall, feet(0, 0), feet(10, 0));
    assert_eq!(status, SearchStatus::Unreachable);
    let short = walker().size(0.4, 0.7).allow_partial(false);
    let (status, _) = search(&mut world, &short, feet(0, 0), feet(10, 0));
    assert_eq!(status, SearchStatus::Complete);
}

#[test]
fn wide_bodies_do_not_fit_one_block_gaps() {
    let mut world = flat();
    wall(&mut world, 5, -32, 31, 3);
    world.fill(BlockPos::new(5, 1, 0), BlockPos::new(5, 3, 0), Cell::EMPTY);
    let narrow = walker().allow_partial(false);
    assert_eq!(
        search(&mut world, &narrow, feet(0, 0), feet(10, 0)).0,
        SearchStatus::Complete
    );
    let wide = walker().size(1.4, 1.0).allow_partial(false);
    assert_eq!(
        search(&mut world, &wide, feet(0, 0), feet(10, 0)).0,
        SearchStatus::Unreachable
    );
}

#[test]
fn diagonals_do_not_cut_corners() {
    let model = walker().compile();
    let mut world = flat();
    world.set(BlockPos::new(1, 1, 0), Cell::FULL);
    world.set(BlockPos::new(1, 2, 0), Cell::FULL);
    let mut moves = Vec::new();
    neighbors(&model, &mut world, BlockPos::new(0, 1, 0), 16, &mut moves);
    assert!(!moves.iter().any(|m| m.pos.x == 1 && m.pos.z == 1));
    assert!(!moves.iter().any(|m| m.pos.x == 1 && m.pos.z == -1));
    assert!(moves.iter().any(|m| m.pos == BlockPos::new(-1, 1, 1)));
}

#[test]
fn water_is_avoided_when_disallowed() {
    let mut world = flat();
    world.fill(
        BlockPos::new(5, 1, -32),
        BlockPos::new(6, 1, 31),
        Cell::EMPTY.with_kind(CellKind::Water),
    );
    let dry = walker().avoid_water().allow_partial(false);
    assert_eq!(
        search(&mut world, &dry, feet(0, 0), feet(10, 0)).0,
        SearchStatus::Unreachable
    );
    assert_eq!(
        search(&mut world, &walker(), feet(0, 0), feet(10, 0)).0,
        SearchStatus::Complete
    );
}

#[test]
fn hazards_and_lava_are_never_crossed_by_default() {
    let mut world = flat();
    world.fill(
        BlockPos::new(5, 1, -32),
        BlockPos::new(5, 1, 31),
        Cell::EMPTY.with_kind(CellKind::Lava),
    );
    world.fill(
        BlockPos::new(6, 0, -32),
        BlockPos::new(6, 0, 31),
        Cell::FULL.with_kind(CellKind::Hazard),
    );
    let profile = walker().allow_partial(false);
    assert_eq!(
        search(&mut world, &profile, feet(0, 0), feet(10, 0)).0,
        SearchStatus::Unreachable
    );
    let reckless = profile.lava_cost(Some(10.0)).hazard_cost(Some(10.0));
    assert_eq!(
        search(&mut world, &reckless, feet(0, 0), feet(10, 0)).0,
        SearchStatus::Complete
    );
}

#[test]
fn unloaded_space_is_impassable() {
    let mut world = flat();
    let profile = walker().allow_partial(false);
    assert_eq!(
        search(&mut world, &profile, feet(0, 0), feet(60, 0)).0,
        SearchStatus::Unreachable
    );
}

#[test]
fn flyers_cross_walls_walkers_cannot() {
    let mut world = flat();
    world.fill(
        BlockPos::new(5, 1, -32),
        BlockPos::new(5, 6, 31),
        Cell::FULL,
    );
    let walker = walker().allow_partial(false);
    assert_eq!(
        search(&mut world, &walker, feet(0, 0), feet(10, 0)).0,
        SearchStatus::Unreachable
    );
    let flyer = NavigationProfile::flyer().allow_partial(false);
    let (status, path) = search(&mut world, &flyer, feet(0, 0), feet(10, 0));
    assert_eq!(status, SearchStatus::Complete);
    assert!(path.points().iter().any(|p| p.y >= 7.0));
}

#[test]
fn swimmers_stay_in_water() {
    let mut world = flat();
    let water = Cell::EMPTY.with_kind(CellKind::Water);
    world.fill(BlockPos::new(-10, 1, -1), BlockPos::new(10, 3, 1), water);
    let swimmer = NavigationProfile::swimmer().allow_partial(false);
    let (status, path) = search(
        &mut world,
        &swimmer,
        BlockPos::new(-8, 1, 0).bottom_center(),
        BlockPos::new(8, 2, 0).bottom_center(),
    );
    assert_eq!(status, SearchStatus::Complete);
    assert!(path.points().iter().all(|p| p.z.abs() < 2.0 && p.y < 4.0));
    let (status, _) = search(
        &mut world,
        &swimmer,
        BlockPos::new(-8, 1, 0).bottom_center(),
        BlockPos::new(8, 1, 5).bottom_center(),
    );
    assert_eq!(status, SearchStatus::Unreachable);
}

#[test]
fn incremental_search_matches_a_one_shot_search() {
    let mut world = flat();
    for x in (-20..20).step_by(4) {
        wall(&mut world, x, -25, 20, 2);
        wall(&mut world, x + 2, -20, 25, 2);
    }
    let profile = walker();
    let request = SearchRequest::new(feet(-25, 0), feet(25, 0));
    let mut one_shot = Path::default();
    let expected = Pathfinder::new().find(&mut world, &profile, request, &mut one_shot);

    let mut pathfinder = Pathfinder::new();
    pathfinder.start(&mut world, &profile.compile(), request);
    let mut ticks = 0;
    let status = loop {
        let mut budget = 64;
        let status = pathfinder.step(&mut world, &mut budget);
        ticks += 1;
        if status.is_done() {
            break status;
        }
        assert_eq!(budget, 0);
    };
    let mut incremental = Path::default();
    pathfinder.write_path(&mut world, &mut incremental);
    assert_eq!(status, expected);
    assert_eq!(incremental, one_shot);
    assert!(ticks > 1);
}

#[test]
fn node_limit_bounds_the_search() {
    let mut world = flat();
    let profile = walker().search_limit(50);
    let mut pathfinder = Pathfinder::new();
    let mut path = Path::default();
    let status = pathfinder.find(
        &mut world,
        &profile,
        SearchRequest::new(feet(-30, -30), feet(30, 30)),
        &mut path,
    );
    assert_eq!(status, SearchStatus::Partial);
    assert!(pathfinder.stats().expanded <= 50);
}

#[test]
fn cached_world_gives_the_same_path_as_the_raw_world() {
    let mut world = flat();
    wall(&mut world, 5, -6, 6, 3);
    let profile = walker();
    let request = SearchRequest::new(feet(0, 0), feet(10, 0));
    let mut raw = Path::default();
    Pathfinder::new().find(&mut world, &profile, request, &mut raw);
    let mut cache = CellCache::new();
    let mut cached = Path::default();
    Pathfinder::new().find(&mut cache.view(&world), &profile, request, &mut cached);
    assert_eq!(raw, cached);
}

#[test]
fn goal_radius_stops_short() {
    let mut world = flat();
    let request = SearchRequest::new(feet(0, 0), feet(10, 0)).within(3.0);
    let mut path = Path::default();
    Pathfinder::new().find(&mut world, &walker(), request, &mut path);
    let end = path.end().expect("path");
    assert!(end.horizontal_distance(feet(10, 0)) <= 3.0);
    assert!(end.horizontal_distance(feet(10, 0)) >= 2.0);
}

#[test]
fn follower_walks_a_simulated_body_to_the_goal() {
    let mut world = flat();
    wall(&mut world, 5, -6, 6, 3);
    let profile = walker();
    let model = profile.compile();
    let mut path = Path::default();
    Pathfinder::new().find(
        &mut world,
        &profile,
        SearchRequest::new(feet(0, 0), feet(10, 0)),
        &mut path,
    );
    let mut follower = PathFollower::default();
    let mut position = feet(0, 0);
    for _ in 0..400 {
        match follower.tick(&path, position, true, 0.2, &model) {
            FollowStatus::Moving(steering) => position += steering.velocity,
            FollowStatus::Arrived => {
                assert!(position.horizontal_distance(feet(10, 0)) < 0.3);
                return;
            }
            FollowStatus::Stuck => panic!("stuck at {position:?}"),
        }
    }
    panic!("never arrived, at {position:?}");
}

#[test]
fn follower_reports_stuck_when_blocked() {
    let model = walker().compile();
    let path = Path::new(vec![feet(5, 0)], true);
    let mut follower = PathFollower::new(10);
    let position = feet(0, 0);
    let mut stuck = false;
    for _ in 0..20 {
        if follower.tick(&path, position, true, 0.2, &model) == FollowStatus::Stuck {
            stuck = true;
            break;
        }
    }
    assert!(stuck);
}

#[test]
fn follower_jumps_onto_rises_above_the_step_height() {
    let model = walker().compile();
    let path = Path::new(vec![BlockPos::new(1, 2, 0).bottom_center()], true);
    let mut follower = PathFollower::default();
    let FollowStatus::Moving(steering) = follower.tick(&path, feet(0, 0), true, 0.2, &model) else {
        panic!("expected steering");
    };
    assert!(steering.jump);
    let FollowStatus::Moving(airborne) = follower.tick(&path, feet(0, 0), false, 0.2, &model)
    else {
        panic!("expected steering");
    };
    assert!(!airborne.jump);
}

#[test]
fn goals_off_the_ground_snap_to_the_nearest_floor() {
    let mut world = flat();
    world.fill(BlockPos::new(6, 1, -3), BlockPos::new(9, 1, 3), Cell::FULL);
    let profile = walker().allow_partial(false);
    let (status, path) = search(&mut world, &profile, feet(0, 0), Vec3::new(3.5, 6.0, 0.5));
    assert_eq!(status, SearchStatus::Complete);
    assert_eq!(path.end(), Some(feet(3, 0)));
    let (status, path) = search(&mut world, &profile, feet(0, 0), Vec3::new(7.5, 0.0, 0.5));
    assert_eq!(status, SearchStatus::Complete);
    assert_eq!(path.end().map(|p| p.y), Some(2.0));
}

#[test]
fn hopeless_short_searches_stop_early() {
    let mut world = flat();
    world.fill(BlockPos::new(8, 1, -2), BlockPos::new(12, 3, 2), Cell::FULL);
    world.fill(
        BlockPos::new(9, 1, -1),
        BlockPos::new(11, 3, 1),
        Cell::EMPTY,
    );
    let profile = walker().allow_partial(false);
    let mut pathfinder = Pathfinder::new();
    let mut path = Path::default();
    let status = pathfinder.find(
        &mut world,
        &profile,
        SearchRequest::new(feet(0, 0), feet(10, 0)),
        &mut path,
    );
    assert_eq!(status, SearchStatus::Unreachable);
    assert!(
        pathfinder.stats().expanded <= 320,
        "{}",
        pathfinder.stats().expanded
    );
    let unbounded = profile.nodes_per_block(0);
    pathfinder.find(
        &mut world,
        &unbounded,
        SearchRequest::new(feet(0, 0), feet(10, 0)),
        &mut path,
    );
    assert!(pathfinder.stats().expanded > 1_000);
}
