use bevy_ecs::prelude::{Component, Entity, Query, With};
use voidmc::components::{PlayerName, PlayerReady, Position};
use voidmc::{
    CollisionRule, Command, CommandBuilder, CommandContext, NameTagVisibility, Objective,
    ScoreFormat, StringArg, Team, TextColor,
};

#[derive(Component)]
pub(super) struct AltitudeBoard;

#[derive(Component)]
pub(super) struct DemoTeam;

pub(super) fn altitude_board() -> impl bevy_ecs::bundle::Bundle {
    (
        Objective::sidebar("altitude")
            .title("Altitude")
            .color(TextColor::Gold)
            .format(ScoreFormat::Styled(TextColor::Aqua)),
        AltitudeBoard,
    )
}

pub(super) fn altitude_system(
    players: Query<(&PlayerName, &Position), With<PlayerReady>>,
    mut boards: Query<&mut Objective, With<AltitudeBoard>>,
) {
    for (name, position) in &players {
        let altitude = position.y.floor() as i32;
        for mut board in &mut boards {
            if board.get(&name.0) != Some(altitude) {
                board.set(&name.0, altitude);
            }
        }
    }
}

pub(super) fn team_command() -> Command {
    CommandBuilder::new("team")
        .description("Example command: join a coloured team (red, blue, gold...) or leave yours")
        .arg_optional("name", StringArg::single_word())
        .flag("leave", Some('l'), "Leave your current team")
        .handler(handle_team)
        .build()
}

fn handle_team(ctx: &mut CommandContext) {
    let player = ctx.entity;
    let name = ctx.get::<String>("name").cloned();
    let leave = ctx.flag("leave");

    let outcome = ctx.with_world_mut(|world| {
        let mut teams = world.query_filtered::<(Entity, &mut Team), With<DemoTeam>>();
        let mut left = None;
        let mut target = None;
        for (entity, mut team) in teams.iter_mut(world) {
            if team.contains(player) && Some(&team.name) != name.as_ref() {
                team.remove(player);
                left = Some(team.name.clone());
            }
            if Some(&team.name) == name.as_ref() {
                target = Some(entity);
            }
        }
        if leave {
            return match left {
                Some(name) => format!("You left team {name}."),
                None => "You are not in a team.".to_string(),
            };
        }
        let Some(name) = name else {
            return "Usage: /team <name> or /team --leave".to_string();
        };
        let team = target.unwrap_or_else(|| {
            let color = TextColor::parse(&name).unwrap_or(TextColor::White);
            world
                .spawn((
                    Team::new(&name)
                        .color(color)
                        .prefix(format!("[{}] ", name.to_uppercase()))
                        .collision(CollisionRule::PushOtherTeams)
                        .name_tags(NameTagVisibility::Always),
                    DemoTeam,
                ))
                .id()
        });
        let mut team = world.get_mut::<Team>(team).unwrap();
        if team.add(player) {
            format!("You joined team {name}.")
        } else {
            format!("You are already in team {name}.")
        }
    });
    ctx.reply(&outcome);
}
