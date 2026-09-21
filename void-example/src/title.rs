use std::sync::Arc;

use voidmc::{Command, CommandBuilder, CommandContext, GreedyStringArg, TextColor, WorldTitles};

pub(super) fn title_command() -> Command {
    CommandBuilder::new("title")
        .description("Example command: show a title (`clear` / `reset` hide it)")
        .arg("title", Arc::new(GreedyStringArg))
        .handler(handle_title)
        .build()
}

fn handle_title(ctx: &mut CommandContext) {
    let title = ctx.get::<String>("title").cloned().unwrap_or_default();
    let player = ctx.entity;
    let reply = ctx.with_world(|world| {
        let titles = WorldTitles::new(world);
        match title.as_str() {
            "clear" => {
                titles.clear(player).send();
                "Title cleared.".to_string()
            }
            "reset" => {
                titles.reset(player).send();
                "Title cleared and times reset.".to_string()
            }
            _ => {
                titles
                    .title(player, &title)
                    .subtitle("")
                    .color(TextColor::Gold)
                    .times(10, 70, 20)
                    .send();
                format!("Title '{title}' sent.")
            }
        }
    });
    ctx.reply(&reply);
}
