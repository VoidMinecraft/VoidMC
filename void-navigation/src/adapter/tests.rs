use bevy_app::{App, Update};
use bevy_ecs::prelude::*;
use voidmc::components::{PlayerDimension, PlayerReady, Position, Velocity};
use voidmc::systems::physics::apply_spawned_entity_physics;
use voidmc::world::{ChunkData, ChunkDimension, ChunkIndex, ChunkPos, ChunkPosition, DimensionId};
use voidmc::{EntityBuilder, EntityKind};
use voidmc_data::v26_1_2::blocks;
use voidmc_protocol::clientbound::chunk::{ChunkHeightmaps, ChunkSection, LightData};

use super::*;
use crate::pathing::{NavigationProfile, Path, Vec3};

#[derive(Resource, Default)]
struct Outcomes(Vec<(Entity, NavigationOutcome)>);

fn record(event: On<NavigationEvent>, mut outcomes: ResMut<Outcomes>) {
    outcomes.0.push((event.entity, event.outcome));
}

fn flat_chunk() -> ChunkData {
    let mut sections: Vec<ChunkSection> = (0..4)
        .map(|_| ChunkSection::filled(blocks::STONE, 0))
        .collect();
    sections.push(ChunkSection::with_floor(blocks::STONE, 0));
    sections.extend((0..19).map(|_| ChunkSection::empty()));
    ChunkData::new(sections, ChunkHeightmaps::empty(), LightData::empty())
}

fn app(settings: NavigationSettings) -> App {
    let mut app = App::new();
    app.init_resource::<ChunkIndex>()
        .init_resource::<Outcomes>()
        .add_observer(record)
        .add_plugins(NavigationPlugin::new().settings(settings))
        .add_systems(
            Update,
            apply_spawned_entity_physics.after(NavigationSystems),
        );
    for x in -3..3 {
        for z in -3..3 {
            let pos = ChunkPos::new(x, z);
            let entity = app
                .world_mut()
                .spawn((
                    ChunkPosition(pos),
                    flat_chunk(),
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

fn set_block(app: &mut App, x: i32, y: i32, z: i32, state: i32) {
    let pos = ChunkPos::new(x.div_euclid(16), z.div_euclid(16));
    let entity = app.world().resource::<ChunkIndex>().0[&(DimensionId::Overworld, pos)];
    app.world_mut()
        .get_mut::<ChunkData>(entity)
        .expect("chunk")
        .set_block(x.rem_euclid(16) as u8, y, z.rem_euclid(16) as u8, state);
}

fn wall(app: &mut App, x: i32, z0: i32, z1: i32) {
    for z in z0..=z1 {
        for y in 1..=3 {
            set_block(app, x, y, z, blocks::STONE);
        }
    }
}

fn mob(app: &mut App, x: f64, z: f64, navigator: Navigator) -> Entity {
    EntityBuilder::new(EntityKind::Zombie)
        .at(x, 1.0, z)
        .gravity(true)
        .block_collision(true)
        .settle_ticks(0)
        .with(navigator)
        .spawn_in(app.world_mut())
        .id()
}

fn player(app: &mut App, x: f64, z: f64) -> Entity {
    app.world_mut()
        .spawn((
            Position { x, y: 1.0, z },
            PlayerDimension(DimensionId::Overworld),
            PlayerReady,
        ))
        .id()
}

fn position(app: &App, entity: Entity) -> Vec3 {
    let p = app.world().get::<Position>(entity).expect("position");
    Vec3::new(p.x, p.y, p.z)
}

fn run(app: &mut App, ticks: usize) {
    for _ in 0..ticks {
        app.update();
    }
}

fn outcomes(app: &App, entity: Entity) -> Vec<NavigationOutcome> {
    app.world()
        .resource::<Outcomes>()
        .0
        .iter()
        .filter(|(e, _)| *e == entity)
        .map(|(_, outcome)| *outcome)
        .collect()
}

fn walker() -> Navigator {
    Navigator::new(NavigationProfile::walker().size(0.6, 1.95).step_height(1.0)).with_speed(0.2)
}

#[test]
fn walks_to_a_target_and_reports_reached() {
    let mut app = app(NavigationSettings::default());
    let target = Vec3::new(12.5, 1.0, 4.5);
    let zombie = mob(
        &mut app,
        0.5,
        0.5,
        walker().with_goal(Goal::move_to(target)),
    );
    run(&mut app, 200);
    assert_eq!(outcomes(&app, zombie), vec![NavigationOutcome::Reached]);
    assert!(position(&app, zombie).horizontal_distance(target) < 0.5);
    let velocity = app.world().get::<Velocity>(zombie).expect("velocity");
    assert_eq!((velocity.x, velocity.z), (0.0, 0.0));
    assert!(
        app.world()
            .get::<Navigator>(zombie)
            .expect("navigator")
            .is_idle()
    );
}

#[test]
fn walks_around_a_wall_through_the_engine_physics() {
    let mut app = app(NavigationSettings::default());
    wall(&mut app, 5, -6, 6);
    let target = Vec3::new(10.5, 1.0, 0.5);
    let zombie = mob(
        &mut app,
        0.5,
        0.5,
        walker().with_goal(Goal::move_to(target)),
    );
    run(&mut app, 300);
    assert_eq!(outcomes(&app, zombie), vec![NavigationOutcome::Reached]);
    assert!(position(&app, zombie).horizontal_distance(target) < 0.5);
}

#[test]
fn climbs_a_one_block_step() {
    let mut app = app(NavigationSettings::default());
    for x in 4..12 {
        for z in -3..3 {
            set_block(&mut app, x, 1, z, blocks::STONE);
        }
    }
    let target = Vec3::new(8.5, 2.0, 0.5);
    let zombie = mob(
        &mut app,
        0.5,
        0.5,
        walker().with_goal(Goal::move_to(target)),
    );
    run(&mut app, 200);
    assert_eq!(outcomes(&app, zombie), vec![NavigationOutcome::Reached]);
    assert!((position(&app, zombie).y - 2.0).abs() < 1.0e-6);
}

#[test]
fn sealed_targets_are_unreachable() {
    let mut app = app(NavigationSettings::default());
    for x in 8..=12 {
        for z in -2..=2 {
            for y in 1..=3 {
                let shell = x == 8 || x == 12 || z == -2 || z == 2 || y == 3;
                set_block(
                    &mut app,
                    x,
                    y,
                    z,
                    if shell { blocks::STONE } else { blocks::AIR },
                );
            }
        }
    }
    let zombie = mob(
        &mut app,
        0.5,
        0.5,
        walker().with_goal(Goal::move_to([10.5, 1.0, 0.5])),
    );
    run(&mut app, 400);
    assert_eq!(outcomes(&app, zombie), vec![NavigationOutcome::Unreachable]);
}

#[test]
fn follows_a_moving_player_and_holds_at_distance() {
    let mut app = app(NavigationSettings::default());
    let steve = player(&mut app, 10.5, 0.5);
    let wolf = mob(&mut app, 0.5, 0.5, walker());
    app.world_mut()
        .get_mut::<Navigator>(wolf)
        .expect("navigator")
        .follow(steve, 2.0);
    run(&mut app, 150);
    let gap = position(&app, wolf).horizontal_distance(position(&app, steve));
    assert!(gap <= 3.0, "gap {gap}");
    assert!(
        !app.world()
            .get::<Navigator>(wolf)
            .expect("navigator")
            .is_moving()
    );

    app.world_mut()
        .get_mut::<Position>(steve)
        .expect("player")
        .z = 14.5;
    run(&mut app, 200);
    let gap = position(&app, wolf).horizontal_distance(position(&app, steve));
    assert!(gap <= 3.0, "gap {gap}");
    assert!(outcomes(&app, wolf).is_empty());
}

#[test]
fn losing_the_followed_entity_interrupts() {
    let mut app = app(NavigationSettings::default());
    let steve = player(&mut app, 10.5, 0.5);
    let wolf = mob(&mut app, 0.5, 0.5, walker());
    app.world_mut()
        .get_mut::<Navigator>(wolf)
        .expect("navigator")
        .follow(steve, 2.0);
    run(&mut app, 5);
    app.world_mut().despawn(steve);
    run(&mut app, 2);
    assert_eq!(outcomes(&app, wolf), vec![NavigationOutcome::Interrupted]);
}

#[test]
fn stopping_interrupts_and_releases_the_velocity() {
    let mut app = app(NavigationSettings::default());
    let zombie = mob(
        &mut app,
        0.5,
        0.5,
        walker().with_goal(Goal::move_to([20.5, 1.0, 0.5])),
    );
    run(&mut app, 10);
    assert_ne!(
        app.world().get::<Velocity>(zombie).expect("velocity").x,
        0.0
    );
    app.world_mut()
        .get_mut::<Navigator>(zombie)
        .expect("navigator")
        .stop();
    run(&mut app, 2);
    assert_eq!(outcomes(&app, zombie), vec![NavigationOutcome::Interrupted]);
    assert_eq!(
        app.world().get::<Velocity>(zombie).expect("velocity").x,
        0.0
    );
}

#[test]
fn patrols_loop_through_their_points() {
    let mut app = app(NavigationSettings::default());
    let points = [[0.5, 1.0, 0.5], [6.5, 1.0, 0.5], [6.5, 1.0, 6.5]];
    let guard = mob(&mut app, 0.5, 0.5, walker().with_goal(Goal::patrol(points)));
    let mut visited = [false; 3];
    for _ in 0..600 {
        app.update();
        let here = position(&app, guard);
        for (index, point) in points.iter().enumerate() {
            if here.horizontal_distance(Vec3::from(*point)) < 0.5 {
                visited[index] = true;
            }
        }
    }
    assert_eq!(visited, [true; 3]);
    assert!(outcomes(&app, guard).is_empty());
    let stats = *app.world().resource::<NavigationStats>();
    assert!(stats.cache_hits >= 3, "{stats:?}");

    set_block(&mut app, 3, 1, 3, blocks::STONE);
    app.update();
    let searches = app.world().resource::<NavigationStats>().searches;
    run(&mut app, 200);
    assert!(app.world().resource::<NavigationStats>().searches > searches);
}

#[test]
fn tiny_budgets_spread_searches_over_ticks() {
    let settings = NavigationSettings {
        expansions_per_tick: 16,
        ..NavigationSettings::default()
    };
    let mut app = app(settings);
    wall(&mut app, 5, -8, 8);
    let mobs: Vec<Entity> = (0..8)
        .map(|i| {
            mob(
                &mut app,
                0.5,
                i as f64 - 3.5,
                walker().with_goal(Goal::move_to([10.5, 1.0, i as f64 - 3.5])),
            )
        })
        .collect();
    app.update();
    let stats = *app.world().resource::<NavigationStats>();
    assert!(stats.expanded_last_tick <= 16);
    assert!(stats.queued >= 7);
    run(&mut app, 900);
    for zombie in mobs {
        assert_eq!(outcomes(&app, zombie), vec![NavigationOutcome::Reached]);
    }
}

#[test]
fn direct_chunk_edits_refresh_cached_cells() {
    let mut app = app(NavigationSettings::default());
    mob(
        &mut app,
        0.5,
        0.5,
        walker().with_goal(Goal::move_to([4.5, 1.0, 0.5])),
    );
    run(&mut app, 2);
    let cell = |app: &App| {
        app.world()
            .resource::<NavigationWorld>()
            .cache(DimensionId::Overworld)
            .and_then(|cache| cache.cached(crate::pathing::BlockPos::new(3, 1, 3)))
    };
    assert_eq!(cell(&app), Some(crate::pathing::Cell::EMPTY));
    set_block(&mut app, 3, 1, 3, blocks::STONE);
    app.update();
    let stone = app
        .world()
        .resource::<NavigationWorld>()
        .table()
        .get(blocks::STONE);
    assert_eq!(cell(&app), Some(stone));
}

#[test]
fn behaviours_pick_the_first_applicable_goal() {
    let mut app = app(NavigationSettings::default());
    let chicken = mob(&mut app, 0.5, 0.5, walker());
    app.world_mut().entity_mut(chicken).insert(
        Behaviours::new()
            .interval(1)
            .with(Behaviour::flee_nearest_player(5.0, 10.0))
            .with(Behaviour::patrol([[0.5, 1.0, 0.5], [4.5, 1.0, 0.5]])),
    );
    run(&mut app, 3);
    let active = |app: &App| {
        app.world()
            .get::<Behaviours>(chicken)
            .expect("behaviours")
            .active()
    };
    assert_eq!(active(&app), Some("patrol"));

    let steve = player(&mut app, 2.5, 0.5);
    run(&mut app, 3);
    assert_eq!(active(&app), Some("flee_player"));
    let mut widest: f64 = 0.0;
    for _ in 0..60 {
        app.update();
        widest = widest.max(position(&app, chicken).horizontal_distance(position(&app, steve)));
    }
    assert!(widest >= 9.5, "widest {widest}");
    assert_eq!(
        outcomes(&app, chicken),
        vec![NavigationOutcome::Interrupted, NavigationOutcome::Reached]
    );
    assert_eq!(active(&app), Some("patrol"));
}

#[test]
fn a_single_point_patrol_settles_instead_of_replanning() {
    let mut app = app(NavigationSettings::default());
    let guard = mob(
        &mut app,
        0.5,
        0.5,
        walker().with_goal(Goal::patrol([[5.5, 1.0, 0.5]])),
    );
    run(&mut app, 80);
    let searches = app.world().resource::<NavigationStats>().searches;
    run(&mut app, 40);
    assert_eq!(app.world().resource::<NavigationStats>().searches, searches);
    assert!(position(&app, guard).horizontal_distance(Vec3::new(5.5, 1.0, 0.5)) < 0.5);
}

#[test]
fn the_default_navigator_climbs_steps_through_the_engine_physics() {
    let mut app = app(NavigationSettings::default());
    for x in 4..12 {
        for z in -3..3 {
            set_block(&mut app, x, 1, z, blocks::STONE);
        }
    }
    for x in 8..12 {
        for z in -3..3 {
            set_block(&mut app, x, 2, z, blocks::STONE);
        }
    }
    let target = Vec3::new(10.5, 3.0, 0.5);
    let zombie = mob(
        &mut app,
        0.5,
        0.5,
        Navigator::default().with_goal(Goal::move_to(target)),
    );
    run(&mut app, 300);
    assert_eq!(outcomes(&app, zombie), vec![NavigationOutcome::Reached]);
    assert!((position(&app, zombie).y - 3.0).abs() < 1.0e-6);
}

#[test]
fn stopping_mid_search_frees_the_planner_at_once() {
    let settings = NavigationSettings {
        expansions_per_tick: 8,
        ..NavigationSettings::default()
    };
    let mut app = app(settings);
    wall(&mut app, 5, -30, 30);
    let slow = mob(
        &mut app,
        0.5,
        0.5,
        walker().with_goal(Goal::move_to([20.5, 1.0, 0.5])),
    );
    let quick = mob(
        &mut app,
        0.5,
        5.5,
        walker().with_goal(Goal::move_to([2.5, 1.0, 5.5])),
    );
    app.update();
    app.world_mut()
        .get_mut::<Navigator>(slow)
        .expect("navigator")
        .stop();
    run(&mut app, 3);
    let navigator = app.world().get::<Navigator>(quick).expect("navigator");
    assert!(navigator.is_moving() || navigator.is_idle());
    assert_eq!(app.world().resource::<NavigationStats>().queued, 0);
}

#[test]
fn a_failed_follow_replan_keeps_the_current_path() {
    let mut app = app(NavigationSettings::default());
    let steve = player(&mut app, 12.5, 0.5);
    let wolf = mob(&mut app, 0.5, 0.5, walker());
    app.world_mut()
        .get_mut::<Navigator>(wolf)
        .expect("navigator")
        .follow(steve, 2.0);
    run(&mut app, 3);
    assert!(
        app.world()
            .get::<Navigator>(wolf)
            .expect("navigator")
            .is_moving()
    );
    for (x, y, z) in [(12, 3, 0), (12, 1, 0), (12, 2, 0)] {
        set_block(&mut app, x, y, z, blocks::STONE);
    }
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        for y in 1..=4 {
            set_block(&mut app, 12 + dx, y, dz, blocks::STONE);
        }
    }
    app.world_mut()
        .get_mut::<Position>(steve)
        .expect("player")
        .y = 4.0;
    run(&mut app, 15);
    let navigator = app.world().get::<Navigator>(wolf).expect("navigator");
    assert!(!navigator.path().is_empty() || navigator.is_moving());
}

#[test]
fn a_huge_unreachable_search_does_not_stall_other_navigators() {
    let mut app = app(NavigationSettings {
        expansions_per_tick: 400,
        ..NavigationSettings::default()
    });
    seal_goal(&mut app);
    let hogs = [hog(&mut app, -10.5), hog(&mut app, 10.5)];
    app.update();
    let others: Vec<Entity> = (0..6)
        .map(|i| {
            let z = -15.5 + i as f64 * 6.0;
            mob(
                &mut app,
                0.5,
                z,
                walker().with_goal(Goal::move_to([8.5, 1.0, z])),
            )
        })
        .collect();
    run(&mut app, 10);
    for &other in &others {
        assert!(
            app.world()
                .get::<Navigator>(other)
                .expect("navigator")
                .is_moving(),
            "a navigator waited behind the huge searches"
        );
    }
    for &hog in &hogs {
        assert!(outcomes(&app, hog).is_empty());
    }
    run(&mut app, 400);
    for &other in &others {
        assert_eq!(outcomes(&app, other), vec![NavigationOutcome::Reached]);
    }
    for &hog in &hogs {
        assert_eq!(outcomes(&app, hog), vec![NavigationOutcome::Unreachable]);
    }
}

fn change_block(app: &mut App, x: i32, y: i32, z: i32, state: i32) {
    set_block(app, x, y, z, state);
    app.world_mut().trigger(voidmc::events::BlockChangeEvent {
        dimension: DimensionId::Overworld,
        position: voidmc_protocol::types::BlockPosition { x, y: y as i16, z },
        old_state: blocks::AIR,
        new_state: state,
        source: None,
    });
}

#[test]
fn a_block_change_refreshes_its_cell_and_keeps_distant_paths_cached() {
    let mut app = app(NavigationSettings::default());
    let goal = [10.5, 1.0, 0.5];
    let send = |app: &mut App| {
        let mob = mob(app, 0.5, 0.5, walker().with_goal(Goal::move_to(goal)));
        run(app, 2);
        app.world_mut().entity_mut(mob).despawn();
        *app.world().resource::<NavigationStats>()
    };
    let before = send(&mut app);
    let sections = app.world().resource::<NavigationWorld>().cached_sections();

    change_block(&mut app, 40, 1, 40, blocks::STONE);
    run(&mut app, 1);
    let after_far = send(&mut app);
    assert_eq!(after_far.cache_hits, before.cache_hits + 1);
    assert_eq!(after_far.searches, before.searches);
    assert_eq!(
        app.world().resource::<NavigationWorld>().cached_sections(),
        sections
    );

    for y in 1..=3 {
        for z in -3..=3 {
            change_block(&mut app, 5, y, z, blocks::STONE);
        }
    }
    run(&mut app, 1);
    let world = app.world().resource::<NavigationWorld>();
    assert_eq!(world.cached_sections(), sections);
    assert_eq!(
        world
            .cache(DimensionId::Overworld)
            .and_then(|cache| cache.cached(crate::pathing::BlockPos::new(5, 2, 0))),
        Some(world.table().get(blocks::STONE))
    );
    let after_near = send(&mut app);
    assert_eq!(after_near.searches, after_far.searches + 1);
    assert_eq!(after_near.cache_hits, after_far.cache_hits);
}

fn seal_goal(app: &mut App) {
    for x in 30..=34 {
        for z in -2..=2 {
            for y in 1..=3 {
                let shell = x == 30 || x == 34 || z == -2 || z == 2 || y == 3;
                set_block(
                    app,
                    x,
                    y,
                    z,
                    if shell { blocks::STONE } else { blocks::AIR },
                );
            }
        }
    }
}

fn hog(app: &mut App, z: f64) -> Entity {
    let profile = NavigationProfile::walker()
        .size(0.6, 1.95)
        .step_height(1.0)
        .search_limit(200_000)
        .nodes_per_block(0)
        .allow_partial(false);
    mob(
        app,
        -20.5,
        z,
        Navigator::new(profile).with_goal(Goal::move_to([32.5, 1.0, 0.5])),
    )
}

fn started(app: &App, entity: Entity) -> bool {
    app.world()
        .get::<Navigator>(entity)
        .is_some_and(|navigator| navigator.is_moving())
        || !outcomes(app, entity).is_empty()
}

fn short_walkers_behind_hogs(count: usize) -> (usize, usize) {
    let mut app = app(NavigationSettings {
        expansions_per_tick: 400,
        ..NavigationSettings::default()
    });
    seal_goal(&mut app);
    let hogs: Vec<Entity> = (0..count)
        .map(|i| hog(&mut app, -30.5 + i as f64 * 6.0))
        .collect();
    let walkers: Vec<Entity> = (0..6)
        .map(|i| {
            let z = -15.5 + i as f64 * 6.0;
            mob(
                &mut app,
                0.5,
                z,
                walker().with_goal(Goal::move_to([8.5, 1.0, z])),
            )
        })
        .collect();
    let mut tick = 0;
    while !walkers.iter().all(|&walker| started(&app, walker)) {
        assert!(tick < 100, "{count} hogs held the walkers for 100 ticks");
        app.update();
        tick += 1;
    }
    let moving = tick;
    while !hogs.iter().all(|&hog| !outcomes(&app, hog).is_empty()) {
        assert!(tick < 20_000, "{count} hogs never finished");
        app.update();
        tick += 1;
    }
    for &hog in &hogs {
        assert_eq!(outcomes(&app, hog), vec![NavigationOutcome::Unreachable]);
    }
    run(&mut app, 60);
    for &walker in &walkers {
        assert_eq!(outcomes(&app, walker), vec![NavigationOutcome::Reached]);
    }
    (moving, tick)
}

#[test]
fn short_searches_are_never_held_up_by_any_number_of_huge_ones() {
    for hogs in [1, 3, 10] {
        let (moving, finished) = short_walkers_behind_hogs(hogs);
        assert!(moving <= 3, "{hogs} hogs: walkers moved at tick {moving}");
        assert!(finished > moving);
    }
}

#[test]
fn a_waiting_long_search_given_a_short_goal_moves_at_once() {
    let mut app = app(NavigationSettings {
        expansions_per_tick: 400,
        ..NavigationSettings::default()
    });
    seal_goal(&mut app);
    let hogs: Vec<Entity> = (0..3)
        .map(|i| hog(&mut app, -30.5 + i as f64 * 6.0))
        .collect();
    run(&mut app, 3);
    assert_eq!(app.world().resource::<NavigationStats>().queued, 3);
    let waiting = hogs[2];
    app.world_mut()
        .get_mut::<Navigator>(waiting)
        .expect("navigator")
        .move_to([-16.5, 1.0, -18.5]);
    run(&mut app, 2);
    assert!(started(&app, waiting));
    assert_eq!(
        outcomes(&app, waiting),
        vec![NavigationOutcome::Interrupted]
    );
}

#[test]
fn a_one_expansion_budget_still_serves_the_main_lane() {
    let mut app = app(NavigationSettings {
        expansions_per_tick: 1,
        ..NavigationSettings::default()
    });
    seal_goal(&mut app);
    let hog = hog(&mut app, 0.5);
    run(&mut app, 40);
    let quick = mob(
        &mut app,
        0.5,
        10.5,
        walker().with_goal(Goal::move_to([1.5, 1.0, 10.5])),
    );
    run(&mut app, 20);
    assert!(started(&app, quick));
    assert!(outcomes(&app, hog).is_empty());
}

#[test]
fn a_cache_hit_deep_in_the_queue_is_served_at_once() {
    let mut app = app(NavigationSettings {
        expansions_per_tick: 50,
        ..NavigationSettings::default()
    });
    seal_goal(&mut app);
    let goal = [10.5, 1.0, 0.5];
    let scout = mob(&mut app, 0.5, 0.5, walker().with_goal(Goal::move_to(goal)));
    run(&mut app, 3);
    assert!(started(&app, scout));
    app.world_mut().entity_mut(scout).despawn();
    for z in [-30.5, -24.5, -18.5] {
        hog(&mut app, z);
    }
    for i in 0..100 {
        let (x, z) = (-40.5 + (i % 10) as f64 * 8.0, 6.5 + (i / 10) as f64 * 4.0);
        mob(
            &mut app,
            x,
            z,
            walker().with_goal(Goal::move_to([x + 6.0, 1.0, z - 3.0])),
        );
    }
    app.update();
    assert!(app.world().resource::<NavigationStats>().queued > 64);
    let hits = app.world().resource::<NavigationStats>().cache_hits;
    let late = mob(&mut app, 0.5, 0.5, walker().with_goal(Goal::move_to(goal)));
    app.update();
    assert_eq!(
        app.world().resource::<NavigationStats>().cache_hits,
        hits + 1
    );
    assert!(started(&app, late));
}

fn send(
    app: &mut App,
    from: [f64; 2],
    goal: [f64; 3],
    navigator: Navigator,
) -> (NavigationStats, Path) {
    let mob = mob(
        app,
        from[0],
        from[1],
        navigator.with_goal(Goal::move_to(goal)),
    );
    run(app, 2);
    let path = app
        .world()
        .get::<Navigator>(mob)
        .expect("navigator")
        .path()
        .clone();
    app.world_mut().entity_mut(mob).despawn();
    (*app.world().resource::<NavigationStats>(), path)
}

fn mixed_mutations_never_leave_stale_cells(event_first: bool) {
    let mut app = app(NavigationSettings::default());
    let goal = [10.5, 1.0, 0.5];
    let (before, _) = send(&mut app, [0.5, 0.5], goal, walker());
    if event_first {
        change_block(&mut app, 3, 1, 12, blocks::STONE);
        wall(&mut app, 5, -6, 6);
    } else {
        wall(&mut app, 5, -6, 6);
        change_block(&mut app, 3, 1, 12, blocks::STONE);
    }
    run(&mut app, 1);
    let world = app.world().resource::<NavigationWorld>();
    let cache = world.cache(DimensionId::Overworld).expect("cache");
    for z in -6..=6 {
        for y in 1..=3 {
            let cell = cache.cached(crate::pathing::BlockPos::new(5, y, z));
            assert!(
                cell.is_none() || cell == Some(world.table().get(blocks::STONE)),
                "stale cell at 5 {y} {z}"
            );
        }
    }
    let (after, path) = send(&mut app, [0.5, 0.5], goal, walker());
    assert_eq!(after.cache_hits, before.cache_hits);
    assert_eq!(after.searches, before.searches + 1);
    assert!(path.points().iter().any(|point| point.z.abs() > 6.0));
}

#[test]
fn an_event_then_a_direct_edit_in_one_tick_leaves_no_stale_cell() {
    mixed_mutations_never_leave_stale_cells(true);
}

#[test]
fn a_direct_edit_then_an_event_in_one_tick_leaves_no_stale_cell() {
    mixed_mutations_never_leave_stale_cells(false);
}

#[test]
fn replacing_a_chunk_refreshes_its_cells_and_paths() {
    let mut app = app(NavigationSettings::default());
    let goal = [10.5, 1.0, 0.5];
    let (before, _) = send(&mut app, [0.5, 0.5], goal, walker());
    let entity =
        app.world().resource::<ChunkIndex>().0[&(DimensionId::Overworld, ChunkPos::new(0, 0))];
    let mut walled = flat_chunk();
    for z in 0..16 {
        for y in 1..=3 {
            walled.set_block(5, y, z, blocks::STONE);
        }
    }
    app.world_mut().entity_mut(entity).insert(walled);
    run(&mut app, 1);
    let world = app.world().resource::<NavigationWorld>();
    assert_eq!(
        world
            .cache(DimensionId::Overworld)
            .and_then(|cache| cache.cached(crate::pathing::BlockPos::new(5, 1, 0))),
        Some(world.table().get(blocks::STONE))
    );
    let (after, _) = send(&mut app, [0.5, 0.5], goal, walker());
    assert_eq!(after.cache_hits, before.cache_hits);
}

#[test]
fn a_wide_body_path_is_invalidated_by_a_block_in_the_next_chunk() {
    let mut app = app(NavigationSettings::default());
    let wide = || Navigator::new(NavigationProfile::walker().size(6.0, 1.95).step_height(1.0));
    let goal = [12.5, 1.0, 13.5];
    let (first, path) = send(&mut app, [4.5, 13.5], goal, wide());
    assert!(path.is_complete());
    let (cached, _) = send(&mut app, [4.5, 13.5], goal, wide());
    assert_eq!(cached.cache_hits, first.cache_hits + 1);
    change_block(&mut app, 8, 1, 16, blocks::STONE);
    run(&mut app, 1);
    let (after, _) = send(&mut app, [4.5, 13.5], goal, wide());
    assert_eq!(after.cache_hits, cached.cache_hits);
    assert_eq!(after.searches, cached.searches + 1);
}
