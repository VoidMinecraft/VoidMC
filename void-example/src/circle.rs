use std::sync::Arc;

use bevy_ecs::prelude::{Component, Entity, Query, With, Without};
use voidmc::components::{PlayerDimension, PlayerName, PlayerReady, Position, Rotation};
use voidmc::world::DimensionId;
use voidmc::{
    Command, CommandBuilder, CommandContext, EntityBuilder, EntityKind, GameProfileArg,
    SummonableEntityArg,
};
use voidmc_data::{Version, is_summonable_entity_type};

const CIRCLE_ENTITY_COUNT: u32 = 36;
const CIRCLE_RADIUS: f64 = 2.0;
const ROTATION_SPEED_DEG: f32 = 2.0;

#[derive(Component)]
pub(super) struct CircleEntity;

#[derive(Component)]
pub(super) struct CircleState {
    angle: f32,
    owner: Entity,
    target: Entity,
}

pub(super) fn circle_command() -> Command {
    CommandBuilder::new("circle")
        .description("Example command: spawn a ring of entities around you or a player")
        .arg_optional("entity", Arc::new(SummonableEntityArg))
        .arg_optional("player", Arc::new(GameProfileArg))
        .flag("stop", Some('s'), "Remove your active circle")
        .handler(handle_circle)
        .build()
}

pub(super) fn circle_system(
    mut circle_entities: Query<
        (&mut Position, &mut Rotation, &mut CircleState),
        With<CircleEntity>,
    >,
    targets: Query<&Position, Without<CircleEntity>>,
) {
    for (mut position, mut rotation, mut state) in circle_entities.iter_mut() {
        state.angle = (state.angle + ROTATION_SPEED_DEG) % 360.0;

        let Ok(target_position) = targets.get(state.target) else {
            continue;
        };

        let angle_rad = (state.angle as f64).to_radians();
        position.x = target_position.x + angle_rad.sin() * CIRCLE_RADIUS;
        position.y = target_position.y;
        position.z = target_position.z + angle_rad.cos() * CIRCLE_RADIUS;

        let angle = state.angle.to_radians();
        let yaw = (-(angle.cos())).atan2(-(angle.sin())).to_degrees();
        rotation.yaw = if yaw < 0.0 { yaw + 360.0 } else { yaw };
    }
}

fn handle_circle(ctx: &mut CommandContext) {
    let executor = ctx.entity;

    if ctx.flag("stop") {
        if dismiss_circle(ctx, executor) {
            ctx.reply("Your circle has dispersed.");
        } else {
            ctx.reply("You have no active circle.");
        }
        return;
    }

    let entity_name = ctx
        .get::<String>("entity")
        .cloned()
        .unwrap_or_else(|| "minecraft:pig".to_string());

    let Some(kind) = EntityKind::from_name(&entity_name) else {
        ctx.reply_error(&format!("Unknown entity type '{}'.", entity_name));
        return;
    };

    if !is_summonable_entity_type(Version::V26_1_2, &entity_name) {
        ctx.reply_error(&format!("Entity type is not summonable: {}", entity_name));
        return;
    }

    let target = if let Some(player_name) = ctx.get::<String>("player").cloned() {
        let found = ctx.with_world_mut(|world| {
            world
                .query_filtered::<(Entity, &PlayerName), With<PlayerReady>>()
                .iter(world)
                .find_map(|(entity, name)| (name.0 == player_name).then_some(entity))
        });

        match found {
            Some(entity) => entity,
            None => {
                ctx.reply_error(&format!("Player '{}' is not online.", player_name));
                return;
            }
        }
    } else {
        executor
    };

    dismiss_circle(ctx, executor);

    let (target_position, dimension) = ctx.with_world(|world| {
        let position = *world.get::<Position>(target).expect("target has Position");
        let dimension = world
            .get::<PlayerDimension>(executor)
            .map(|dimension| dimension.0)
            .unwrap_or(DimensionId::Overworld);
        (position, dimension)
    });

    for index in 0..CIRCLE_ENTITY_COUNT {
        let angle = (index * 10) as f32;
        let angle_rad = angle.to_radians() as f64;
        let x = target_position.x + angle_rad.sin() * CIRCLE_RADIUS;
        let y = target_position.y;
        let z = target_position.z + angle_rad.cos() * CIRCLE_RADIUS;

        ctx.with_world_mut(|world| {
            EntityBuilder::new(kind)
                .at(x, y, z)
                .in_dimension(dimension)
                .settle_ticks(5)
                .spawn_in(world)
                .insert((
                    CircleEntity,
                    CircleState {
                        angle,
                        owner: executor,
                        target,
                    },
                ));
        });
    }

    let label = if target == executor {
        "you".to_string()
    } else {
        ctx.get::<String>("player")
            .cloned()
            .unwrap_or_else(|| "the target".to_string())
    };

    ctx.reply(&format!(
        "36 entities now orbit around {}. Use /circle --stop to dismiss them.",
        label
    ));
}

fn dismiss_circle(ctx: &mut CommandContext, executor: Entity) -> bool {
    let existing: Vec<Entity> = ctx.with_world_mut(|world| {
        world
            .query_filtered::<(Entity, &CircleState), With<CircleEntity>>()
            .iter(world)
            .filter_map(|(entity, state)| (state.owner == executor).then_some(entity))
            .collect()
    });

    if existing.is_empty() {
        return false;
    }

    ctx.with_world_mut(|world| {
        for entity in existing {
            world.despawn(entity);
        }
    });

    true
}
