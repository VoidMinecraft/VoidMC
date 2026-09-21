use std::sync::Arc;

use voidmc::{
    Command, CommandBuilder, CommandContext, GreedyStringArg, StringArg, TextColor, WorldTitles,
};

pub(super) fn title_command() -> Command {
    CommandBuilder::new("title")
        .description("Example command: show a title (`clear` / `reset` hide it)")
        .arg("title", StringArg::single_word())
        .arg_optional("subtitle", Arc::new(GreedyStringArg))
        .handler(handle_title)
        .build()
}

fn handle_title(ctx: &mut CommandContext) {
    let title = ctx.get::<String>("title").cloned().unwrap_or_default();
    let subtitle = ctx.get::<String>("subtitle").cloned();
    let player = ctx.entity;
    ctx.with_world(|world| {
        let titles = WorldTitles::new(world);
        match title.as_str() {
            "clear" => titles.clear(player).send(),
            "reset" => titles.reset(player).send(),
            _ => {
                let mut request = titles
                    .title(player, &title)
                    .color(TextColor::Gold)
                    .times(10, 70, 20);
                if let Some(subtitle) = subtitle {
                    request = request.subtitle(subtitle).subtitle_color(TextColor::Gray);
                }
                request.send();
            }
        }
    });
    ctx.reply(&format!("Title '{title}' sent."));
}
