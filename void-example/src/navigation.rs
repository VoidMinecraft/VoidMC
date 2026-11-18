use bevy_ecs::prelude::*;
use voidmc::components::{PlayerDimension, Position};
use voidmc::events::PlayerReadyEvent;
use voidmc::world::DimensionId;
use voidmc::{Command, CommandBuilder, CommandContext, CustomName, EntityBuilder, EntityKind};
use voidmc_navigation::{Behaviour, Behaviours, Goal, Navigator, walking_profile};

#[derive(Resource, Default)]
pub(super) struct DemoSpawned(bool);

pub(super) fn navdemo_command() -> Command {
    CommandBuilder::new("navdemo")
        .description("Example command: spawn mobs that patrol, follow and flee around you")
        .handler(handle_navdemo)
        .build()
}

fn handle_navdemo(ctx: &mut CommandContext) {
    let player = ctx.entity;
    let spawned = ctx.with_world_mut(|world| spawn_demo(world, player));
    if spawned {
        ctx.reply(
            "Two pigs patrol, a wolf follows the nearest player and chickens flee. \
             Hold a blaze rod (/nav wand): right-click a mob to select it, a block to send it.",
        );
    } else {
        ctx.reply_error("You have no position.");
    }
}

pub(super) fn spawn_on_first_join(
    event: On<PlayerReadyEvent>,
    mut spawned: ResMut<DemoSpawned>,
    mut commands: Commands,
) {
    if spawned.0 {
        return;
    }
    spawned.0 = true;
    let player = event.entity;
    commands.queue(move |world: &mut World| {
        spawn_demo(world, player);
    });
}

fn spawn_demo(world: &mut World, player: Entity) -> bool {
    let Some(&Position { x, y, z }) = world.get::<Position>(player) else {
        return false;
    };
    let dimension = world
        .get::<PlayerDimension>(player)
        .map_or(DimensionId::Overworld, |d| d.0);
    let (x, z) = (x.floor() + 0.5, z.floor() + 0.5);
    let walker = |kind: EntityKind, dx: f64, dz: f64| {
        EntityBuilder::new(kind)
            .at(x + dx, y, z + dz)
            .in_dimension(dimension)
            .gravity(true)
            .block_collision(true)
    };

    for (index, offset) in [6.0, -6.0].into_iter().enumerate() {
        let square = [
            [x + offset, y, z + 6.0],
            [x + offset + 8.0, y, z + 6.0],
            [x + offset + 8.0, y, z + 14.0],
            [x + offset, y, z + 14.0],
        ];
        walker(EntityKind::Pig, offset, 6.0)
            .with(CustomName::new(format!("Guard {}", index + 1)))
            .with(
                Navigator::new(walking_profile(EntityKind::Pig))
                    .with_speed(0.12)
                    .with_goal(Goal::patrol(square)),
            )
            .spawn_in(world);
    }

    walker(EntityKind::Wolf, -3.0, -3.0)
        .with(CustomName::new("Buddy"))
        .with(Navigator::new(walking_profile(EntityKind::Wolf)).with_speed(0.25))
        .with(
            Behaviours::new()
                .with(Behaviour::follow_nearest_player(24.0, 3.0))
                .with(Behaviour::wander([x, y, z], 10.0)),
        )
        .spawn_in(world);

    for i in 0..3 {
        let angle = i as f64 * 2.1;
        walker(
            EntityKind::Chicken,
            angle.cos() * 4.0,
            angle.sin() * 4.0 - 8.0,
        )
        .with(Navigator::new(walking_profile(EntityKind::Chicken)).with_speed(0.2))
        .with(
            Behaviours::new()
                .interval(5)
                .with(Behaviour::flee_nearest_player(5.0, 12.0))
                .with(Behaviour::wander([x, y, z - 8.0], 8.0)),
        )
        .spawn_in(world);
    }
    true
}
