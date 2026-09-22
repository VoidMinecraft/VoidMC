use std::sync::Arc;

use bevy_ecs::prelude::{Component, Entity, Query, With};
use voidmc::components::{PlayerDimension, Position};
use voidmc::world::DimensionId;
use voidmc::{
    BlockDisplay, Command, CommandBuilder, CommandContext, CustomName, Display, DisplayTransform,
    EntityBuilder, EntityKind, Glowing, GreedyStringArg, Hidden, Invisible, ItemDisplay, ItemStack,
    NoGravity, Passengers, StringArg, SummonableEntityArg, TextAlignment, TextDisplay,
};
use voidmc_data::v26_1_2::blocks;
use voidmc_data::{Version, is_summonable_entity_type};

const SHIELD_SEGMENTS: usize = 8;
const SHIELD_RADIUS: f32 = 1.5;
const SHIELD_TURN_DEG: f32 = 3.0;

#[derive(Component)]
pub(super) struct DemoDisplay {
    owner: Entity,
}

#[derive(Component)]
pub(super) struct ShieldSegment {
    angle: f32,
}

pub(super) fn spawn_command() -> Command {
    CommandBuilder::new("spawn")
        .description("Example command: spawn an entity with metadata sugar")
        .arg_optional("entity", Arc::new(SummonableEntityArg))
        .flag_value(
            "name",
            Some('n'),
            "Custom name shown above the entity",
            Arc::new(GreedyStringArg),
        )
        .flag("glow", Some('g'), "Glowing outline")
        .flag("invisible", Some('i'), "Invisible")
        .flag("float", Some('f'), "No gravity")
        .handler(handle_spawn)
        .build()
}

fn handle_spawn(ctx: &mut CommandContext) {
    let entity_name = ctx
        .get::<String>("entity")
        .cloned()
        .unwrap_or_else(|| "minecraft:zombie".to_string());
    let Some(kind) = EntityKind::from_name(&entity_name) else {
        ctx.reply_error(&format!("Unknown entity type '{}'.", entity_name));
        return;
    };
    if !is_summonable_entity_type(Version::V26_1_2, &entity_name) {
        ctx.reply_error(&format!("Entity type is not summonable: {}", entity_name));
        return;
    }

    let executor = ctx.entity;
    let name = ctx.flag_value::<String>("name").cloned();
    let (glow, invisible, float) = (ctx.flag("glow"), ctx.flag("invisible"), ctx.flag("float"));

    ctx.with_world_mut(|world| {
        let (position, dimension) = origin(world, executor);
        let mut builder = EntityBuilder::new(kind)
            .position(position)
            .in_dimension(dimension)
            .gravity(!float);
        if let Some(name) = name {
            builder = builder.with(CustomName::new(name));
        }
        if glow {
            builder = builder.with(Glowing);
        }
        if invisible {
            builder = builder.with(Invisible);
        }
        if float {
            builder = builder.with(NoGravity);
        }
        builder.spawn_in(world);
    });
    ctx.reply(&format!("Spawned {}.", entity_name));
}

pub(super) fn display_command() -> Command {
    CommandBuilder::new("display")
        .description("Example command: block/item/text display entities (shield, sign, ride, hide)")
        .arg("mode", StringArg::single_word())
        .arg_optional("text", Arc::new(GreedyStringArg))
        .handler(handle_display)
        .build()
}

fn handle_display(ctx: &mut CommandContext) {
    let mode = ctx.get::<String>("mode").cloned().unwrap_or_default();
    let text = ctx.get::<String>("text").cloned();
    let executor = ctx.entity;

    let reply = ctx.with_world_mut(|world| {
        let (position, dimension) = origin(world, executor);
        match mode.as_str() {
            "shield" => {
                for i in 0..SHIELD_SEGMENTS {
                    let angle = i as f32 * (360.0 / SHIELD_SEGMENTS as f32);
                    EntityBuilder::new(EntityKind::BlockDisplay)
                        .position(position)
                        .in_dimension(dimension)
                        .with_bundle((
                            BlockDisplay(blocks::CYAN_STAINED_GLASS),
                            Display::default()
                                .transform(shield_transform(angle))
                                .interpolation_ticks(2)
                                .brightness(15, 15)
                                .view_range(2.0)
                                .culling_box(2.0 * SHIELD_RADIUS + 1.0, 2.0),
                            ShieldSegment { angle },
                            DemoDisplay { owner: executor },
                        ))
                        .spawn_in(world);
                }
                "Shield raised. /display clear removes it."
            }
            "sign" => {
                EntityBuilder::new(EntityKind::TextDisplay)
                    .at(position.x, position.y + 2.2, position.z)
                    .in_dimension(dimension)
                    .with_bundle((
                        TextDisplay::new(text.unwrap_or_else(|| "Hello from void".into()))
                            .shadow()
                            .alignment(TextAlignment::Center),
                        Display::default().billboard(voidmc::Billboard::Center),
                        DemoDisplay { owner: executor },
                    ))
                    .spawn_in(world);
                "Sign placed above you."
            }
            "item" => {
                EntityBuilder::new(EntityKind::ItemDisplay)
                    .at(position.x, position.y + 1.0, position.z)
                    .in_dimension(dimension)
                    .with_bundle((
                        ItemDisplay::new(ItemStack::new(
                            voidmc::ItemId::from_name("minecraft:diamond_sword").unwrap(),
                            1,
                        )),
                        Display::default()
                            .transform(DisplayTransform::default().uniform_scale(2.0)),
                        DemoDisplay { owner: executor },
                    ))
                    .spawn_in(world);
                "Item display placed."
            }
            "ride" => {
                let pig = EntityBuilder::new(EntityKind::Pig)
                    .position(position)
                    .in_dimension(dimension)
                    .gravity(true)
                    .block_collision(true)
                    .with(DemoDisplay { owner: executor })
                    .spawn_in(world)
                    .id();
                let chicken = EntityBuilder::new(EntityKind::Chicken)
                    .position(position)
                    .in_dimension(dimension)
                    .with_bundle((
                        CustomName::new("Passenger"),
                        DemoDisplay { owner: executor },
                    ))
                    .spawn_in(world)
                    .id();
                world.entity_mut(pig).insert(Passengers::new([chicken]));
                "A chicken now rides a pig."
            }
            "hide" => {
                let owned = owned_displays(world, executor);
                let hide = owned.iter().any(|e| world.get::<Hidden>(*e).is_none());
                for entity in owned {
                    if hide {
                        world.entity_mut(entity).insert(Hidden);
                    } else {
                        world.entity_mut(entity).remove::<Hidden>();
                    }
                }
                if hide {
                    "Your displays are hidden. /display hide shows them again."
                } else {
                    "Your displays are visible again."
                }
            }
            "clear" => {
                for entity in owned_displays(world, executor) {
                    world.despawn(entity);
                }
                "Your displays are gone."
            }
            _ => "Usage: /display <shield|sign|item|ride|hide|clear> [text]",
        }
    });
    ctx.reply(reply);
}

pub(super) fn shield_system(
    mut segments: Query<(
        &mut Display,
        &mut Position,
        &mut ShieldSegment,
        &DemoDisplay,
    )>,
    owners: Query<
        &Position,
        (
            With<PlayerDimension>,
            bevy_ecs::prelude::Without<ShieldSegment>,
        ),
    >,
) {
    for (mut display, mut position, mut segment, demo) in segments.iter_mut() {
        segment.angle = (segment.angle + SHIELD_TURN_DEG) % 360.0;
        if let Ok(owner) = owners.get(demo.owner)
            && *position != *owner
        {
            *position = *owner;
        }
        display.transform = shield_transform(segment.angle);
    }
}

fn shield_transform(angle_deg: f32) -> DisplayTransform {
    let angle = angle_deg.to_radians();
    let half = angle / 2.0;
    DisplayTransform::default()
        .translation(
            angle.sin() * SHIELD_RADIUS - 0.25,
            0.5,
            angle.cos() * SHIELD_RADIUS - 0.25,
        )
        .uniform_scale(0.5)
        .left_rotation([0.0, half.sin(), 0.0, half.cos()])
}

fn owned_displays(world: &mut bevy_ecs::prelude::World, executor: Entity) -> Vec<Entity> {
    world
        .query::<(Entity, &DemoDisplay)>()
        .iter(world)
        .filter_map(|(e, d)| (d.owner == executor).then_some(e))
        .collect()
}

fn origin(world: &bevy_ecs::prelude::World, executor: Entity) -> (Position, DimensionId) {
    let position = world.get::<Position>(executor).copied().unwrap_or_default();
    let dimension = world
        .get::<PlayerDimension>(executor)
        .map(|d| d.0)
        .unwrap_or(DimensionId::Overworld);
    (position, dimension)
}
