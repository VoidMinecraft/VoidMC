use bevy_ecs::prelude::{Commands, Component, Entity, Query};
use voidmc::events::PlayerQuitEvent;
use voidmc::{
    BossBar, BossBarColor, BossBarDivision, BossBarViewers, Command, CommandBuilder,
    CommandContext, FloatArg, On,
};

#[derive(Component)]
pub(super) struct DemoBossBar(Entity);

pub(super) fn bossbar_command() -> Command {
    CommandBuilder::new("bossbar")
        .description("Example command: show a personal boss bar, set its progress, or remove it")
        .arg_optional("progress", FloatArg::new(0.0, 1.0))
        .flag("remove", Some('r'), "Remove your boss bar")
        .handler(handle_bossbar)
        .build()
}

fn handle_bossbar(ctx: &mut CommandContext) {
    let player = ctx.entity;
    let progress = ctx.get::<f32>("progress").copied();
    let remove = ctx.flag("remove");

    let outcome = ctx.with_world_mut(|world| {
        let existing = world.get::<DemoBossBar>(player).map(|bar| bar.0);
        match (remove, existing) {
            (true, Some(bar)) => {
                world.despawn(bar);
                world.entity_mut(player).remove::<DemoBossBar>();
                "Boss bar removed."
            }
            (true, None) => "You have no boss bar.",
            (false, Some(bar)) => {
                let mut bar = world.get_mut::<BossBar>(bar).unwrap();
                bar.set_progress(progress.unwrap_or(1.0));
                "Boss bar updated."
            }
            (false, None) => {
                let bar = world
                    .spawn((
                        BossBar::new("Void Example")
                            .color(BossBarColor::Purple)
                            .division(BossBarDivision::Notches10)
                            .progress(progress.unwrap_or(1.0)),
                        BossBarViewers::new([player]),
                    ))
                    .id();
                world.entity_mut(player).insert(DemoBossBar(bar));
                "Boss bar shown. Try /bossbar 0.5 and /bossbar --remove."
            }
        }
    });
    ctx.reply(outcome);
}

pub(super) fn despawn_bossbar_on_quit(
    event: On<PlayerQuitEvent>,
    owners: Query<&DemoBossBar>,
    mut commands: Commands,
) {
    if let Ok(bar) = owners.get(event.entity) {
        commands.entity(bar.0).despawn();
    }
}
