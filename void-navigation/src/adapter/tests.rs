use bevy_app::{App, Update};
use bevy_ecs::prelude::*;
use voidmc::components::{PlayerDimension, PlayerReady, Position, Velocity};
use voidmc::systems::physics::apply_spawned_entity_physics;
use voidmc::world::{ChunkData, ChunkDimension, ChunkIndex, ChunkPos, ChunkPosition, DimensionId};
use voidmc::{EntityBuilder, EntityKind};
use voidmc_data::v26_1_2::blocks;
use voidmc_protocol::clientbound::chunk::{ChunkHeightmaps, ChunkSection, LightData};

use super::*;
use crate::pathing::{NavigationProfile, SectionPos, Vec3};

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
fn block_changes_invalidate_cached_sections() {
    let mut app = app(NavigationSettings::default());
    let zombie = mob(
        &mut app,
        0.5,
        0.5,
        walker().with_goal(Goal::move_to([4.5, 1.0, 0.5])),
    );
    run(&mut app, 2);
    let section = SectionPos::new(0, 0, 0);
    let cached = |app: &App| {
        app.world()
            .resource::<NavigationWorld>()
            .cache(DimensionId::Overworld)
            .is_some_and(|cache| cache.is_cached(section))
    };
    assert!(cached(&app));
    set_block(&mut app, 3, 1, 3, blocks::STONE);
    app.update();
    let _ = zombie;
    assert!(!cached(&app) || app.world().resource::<NavigationStats>().searches > 1);
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
