use std::sync::Arc;

use bevy_ecs::prelude::{Entity, Resource};
use voidmc::{
    Command, CommandBuilder, CommandContext, GreedyStringArg, StringArg, TabEntry, TabList,
    TextColor,
};

#[derive(Resource)]
struct DemoTabList(Entity);

pub(super) fn tablist_command() -> Command {
    CommandBuilder::new("tablist")
        .description("Example command: set the tab list header and footer everyone sees")
        .arg_optional("header", StringArg::single_word())
        .arg_variadic("footer", Arc::new(GreedyStringArg))
        .flag("clear", Some('c'), "Remove the header and footer")
        .handler(handle_tablist)
        .build()
}

fn handle_tablist(ctx: &mut CommandContext) {
    let header = ctx.get::<String>("header").cloned().unwrap_or_default();
    let footer = ctx.get::<String>("footer").cloned().unwrap_or_default();
    let clear = ctx.flag("clear");

    let outcome = ctx.with_world_mut(|world| {
        let existing = world.get_resource::<DemoTabList>().map(|list| list.0);
        match (clear, existing) {
            (true, Some(list)) => {
                world.despawn(list);
                world.remove_resource::<DemoTabList>();
                "Tab list header and footer removed."
            }
            (true, None) => "No header or footer set.",
            (false, Some(list)) => {
                let mut list = world.get_mut::<TabList>(list).unwrap();
                list.header = header;
                list.footer = footer;
                "Tab list updated."
            }
            (false, None) => {
                let list = world
                    .spawn(
                        TabList::new()
                            .header(header)
                            .header_color(TextColor::Gold)
                            .footer(footer)
                            .footer_color(TextColor::Gray),
                    )
                    .id();
                world.insert_resource(DemoTabList(list));
                "Tab list set. Try /tablist --clear."
            }
        }
    });
    ctx.reply(outcome);
}

pub(super) fn nick_command() -> Command {
    CommandBuilder::new("nick")
        .description("Example command: change how your name shows in the tab list")
        .arg_optional("name", StringArg::single_word())
        .flag("hide", Some('h'), "Hide yourself from the tab list")
        .flag("reset", Some('r'), "Back to your real name")
        .handler(handle_nick)
        .build()
}

fn handle_nick(ctx: &mut CommandContext) {
    let name = ctx.get::<String>("name").cloned();
    let hide = ctx.flag("hide");
    let reset = ctx.flag("reset");
    let player = ctx.entity;

    let outcome = ctx.with_world_mut(|world| {
        let Some(mut entry) = world.get_mut::<TabEntry>(player) else {
            return "You have no tab list entry yet.";
        };
        if reset {
            *entry = TabEntry::new();
            return "Tab list entry reset.";
        }
        if let Some(name) = name {
            entry.display_name = Some(name);
            entry.color = TextColor::Aqua;
        }
        entry.listed = !hide;
        "Tab list entry updated."
    });
    ctx.reply(outcome);
}
