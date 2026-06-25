use bevy_app::{App, Update};
use bevy_ecs::prelude::*;

const ENTITY_COUNT: usize = 1_000;
const TICKS: usize = 120;

#[derive(Component)]
struct Position {
    x: f32,
}

#[derive(Component)]
struct Velocity {
    x: f32,
}

#[derive(Component)]
struct Wanders;

#[derive(Resource, Default)]
struct TickCount(usize);

fn main() {
    let bevy_result = run_bevy_ecs();
    let manual_result = run_manual_loop();

    println!("ECS modularity POC");
    println!("entities={ENTITY_COUNT}, ticks={TICKS}");
    println!();
    println!("| Approach | Final position sum | Notes |");
    println!("|---|---:|---|");
    println!(
        "| Bevy ECS | {:.2} | Selected: systems, resources, schedules, and plugins compose naturally |",
        bevy_result
    );
    println!(
        "| Manual loop | {:.2} | Plausible for a prototype, but gameplay features become tightly coupled |",
        manual_result
    );
}

fn run_bevy_ecs() -> f32 {
    let mut app = App::new();
    app.insert_resource(TickCount::default())
        .add_systems(Update, (wander_system, movement_system, tick_system));

    for id in 0..ENTITY_COUNT {
        app.world_mut()
            .spawn((Position { x: id as f32 }, Velocity { x: 1.0 }, Wanders));
    }

    for _ in 0..TICKS {
        app.update();
    }

    let mut query = app.world_mut().query::<&Position>();
    query.iter(app.world()).map(|position| position.x).sum()
}

fn wander_system(mut query: Query<&mut Velocity, With<Wanders>>, tick: Res<TickCount>) {
    for mut velocity in &mut query {
        velocity.x = if tick.0.is_multiple_of(2) { 1.0 } else { -0.5 };
    }
}

fn movement_system(mut query: Query<(&mut Position, &Velocity)>) {
    for (mut position, velocity) in &mut query {
        position.x += velocity.x;
    }
}

fn tick_system(mut tick: ResMut<TickCount>) {
    tick.0 += 1;
}

struct ManualEntity {
    position_x: f32,
    velocity_x: f32,
    wanders: bool,
}

fn run_manual_loop() -> f32 {
    let mut entities: Vec<_> = (0..ENTITY_COUNT)
        .map(|id| ManualEntity {
            position_x: id as f32,
            velocity_x: 1.0,
            wanders: true,
        })
        .collect();

    for tick in 0..TICKS {
        for entity in &mut entities {
            if entity.wanders {
                entity.velocity_x = if tick.is_multiple_of(2) { 1.0 } else { -0.5 };
            }
        }

        for entity in &mut entities {
            entity.position_x += entity.velocity_x;
        }
    }

    entities.iter().map(|entity| entity.position_x).sum()
}
