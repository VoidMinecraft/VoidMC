use std::time::Duration;

use bevy_ecs::prelude::{Component, With};
use voidmc::{Command, CommandBuilder, CommandContext, DoubleArg, IntegerArg, WorldBorder};

#[derive(Component)]
struct DemoBorder;

pub(super) fn border_command() -> Command {
    CommandBuilder::new("border")
        .description("Example command: show a world border, resize it over time, or remove it")
        .arg_optional("diameter", DoubleArg::new(1.0, 59_999_968.0))
        .arg_optional("seconds", IntegerArg::new(0, 86_400))
        .flag("remove", Some('r'), "Remove the demo border")
        .handler(handle_border)
        .build()
}

fn handle_border(ctx: &mut CommandContext) {
    let diameter = ctx.get::<f64>("diameter").copied();
    let seconds = ctx.get::<i32>("seconds").copied().unwrap_or(0);
    let remove = ctx.flag("remove");

    let outcome = ctx.with_world_mut(|world| {
        let existing = world
            .query_filtered::<bevy_ecs::prelude::Entity, With<DemoBorder>>()
            .iter(world)
            .next();
        match (remove, existing, diameter) {
            (true, Some(border), _) => {
                world.despawn(border);
                "Border removed."
            }
            (true, None, _) => "There is no demo border.",
            (false, Some(border), Some(diameter)) => {
                let mut border = world.get_mut::<WorldBorder>(border).unwrap();
                if seconds == 0 {
                    border.set_diameter(diameter);
                } else {
                    border.shrink_to(diameter, Duration::from_secs(seconds as u64));
                }
                "Border resized."
            }
            (false, Some(_), None) => "The demo border is already shown.",
            (false, None, diameter) => {
                world.spawn((
                    WorldBorder::new()
                        .center(0.0, 0.0)
                        .diameter(diameter.unwrap_or(512.0))
                        .warning_blocks(8)
                        .warning_time(Duration::from_secs(15)),
                    DemoBorder,
                ));
                "Border shown to everyone. Try /border 64 120 and /border --remove."
            }
        }
    });
    ctx.reply(outcome);
}
