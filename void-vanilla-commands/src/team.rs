use std::collections::BTreeSet;
use std::sync::Arc;

use bevy_ecs::prelude::*;
use voidmc::components::{PlayerName, PlayerReady};
use voidmc::{
    BoolArg, CollisionRule, ColorArg, Command, CommandBuilder, CommandContext, NameTagVisibility,
    Team, TeamColor, TextColor,
};

use crate::args::{ComponentArg, NameArg, ScoreHolderArg, TeamArg, find_team};
use crate::text::StyledText;
use crate::{Access, respond};

/// `/team modify <team> deathMessageVisibility`. Stored on the team entity
/// for game code to read; the engine itself sends no death messages.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeathMessageVisibility(pub NameTagVisibility);

const VISIBILITIES: [(&str, NameTagVisibility); 4] = [
    ("never", NameTagVisibility::Never),
    ("hideForOtherTeams", NameTagVisibility::HideForOtherTeams),
    ("hideForOwnTeam", NameTagVisibility::HideForOwnTeam),
    ("always", NameTagVisibility::Always),
];

const COLLISIONS: [(&str, CollisionRule); 4] = [
    ("always", CollisionRule::Always),
    ("never", CollisionRule::Never),
    ("pushOtherTeams", CollisionRule::PushOtherTeams),
    ("pushOwnTeam", CollisionRule::PushOwnTeam),
];

type Outcome = Result<String, String>;

#[derive(Component)]
struct CommandTeam;

pub fn team_command(access: Access) -> Command {
    let choices =
        |name: &str,
         values: &'static [(&'static str, NameTagVisibility)],
         set: fn(&mut World, Entity, &'static str, NameTagVisibility) -> Outcome| {
            values
                .iter()
                .fold(CommandBuilder::new(name), |builder, &(label, value)| {
                    builder.subcommand(CommandBuilder::new(label).handler(
                        move |ctx: &mut CommandContext| {
                            let team = team_arg(ctx);
                            let outcome =
                                ctx.with_world_mut(|world| set(world, team, label, value));
                            respond(ctx, outcome);
                        },
                    ))
                })
        };
    let collisions = COLLISIONS.iter().fold(
        CommandBuilder::new("collisionRule"),
        |builder, &(label, rule)| {
            builder.subcommand(CommandBuilder::new(label).handler(
                move |ctx: &mut CommandContext| {
                    let team = team_arg(ctx);
                    let outcome =
                        ctx.with_world_mut(|world| set_collision(world, team, label, rule));
                    respond(ctx, outcome);
                },
            ))
        },
    );

    CommandBuilder::new("team")
        .description("Manage teams")
        .requires(move |world, player| access.allows(world, player))
        .subcommand(
            CommandBuilder::new("add")
                .arg("team", Arc::new(NameArg))
                .arg_variadic("displayName", Arc::new(ComponentArg))
                .handler(add),
        )
        .subcommand(
            CommandBuilder::new("remove")
                .arg("team", Arc::new(TeamArg))
                .handler(remove),
        )
        .subcommand(
            CommandBuilder::new("empty")
                .arg("team", Arc::new(TeamArg))
                .handler(empty),
        )
        .subcommand(
            CommandBuilder::new("join")
                .arg("team", Arc::new(TeamArg))
                .arg_optional("members", ScoreHolderArg::multiple())
                .handler(join),
        )
        .subcommand(
            CommandBuilder::new("leave")
                .arg("members", ScoreHolderArg::multiple())
                .handler(leave),
        )
        .subcommand(
            CommandBuilder::new("list")
                .arg_optional("team", Arc::new(TeamArg))
                .handler(list),
        )
        .subcommand(
            CommandBuilder::new("modify")
                .arg("team", Arc::new(TeamArg))
                .subcommand(
                    CommandBuilder::new("displayName")
                        .arg_variadic_required("displayName", Arc::new(ComponentArg))
                        .handler(modify_display_name),
                )
                .subcommand(
                    CommandBuilder::new("color")
                        .arg("value", Arc::new(ColorArg))
                        .handler(modify_color),
                )
                .subcommand(
                    CommandBuilder::new("friendlyFire")
                        .arg("allowed", Arc::new(BoolArg))
                        .handler(modify_friendly_fire),
                )
                .subcommand(
                    CommandBuilder::new("seeFriendlyInvisibles")
                        .arg("allowed", Arc::new(BoolArg))
                        .handler(modify_see_invisibles),
                )
                .subcommand(choices("nametagVisibility", &VISIBILITIES, set_name_tags))
                .subcommand(choices(
                    "deathMessageVisibility",
                    &VISIBILITIES,
                    set_death_messages,
                ))
                .subcommand(collisions)
                .subcommand(
                    CommandBuilder::new("prefix")
                        .arg_variadic_required("prefix", Arc::new(ComponentArg))
                        .handler(modify_prefix),
                )
                .subcommand(
                    CommandBuilder::new("suffix")
                        .arg_variadic_required("suffix", Arc::new(ComponentArg))
                        .handler(modify_suffix),
                ),
        )
        .build()
}

/// Teams list members by name so they survive reconnects; put the player's
/// entity back so `Team::contains` holds again once they are ready.
pub(crate) fn rejoin_teams(
    event: On<Add, PlayerReady>,
    names: Query<&PlayerName>,
    mut teams: Query<&mut Team>,
) {
    let Ok(name) = names.get(event.entity) else {
        return;
    };
    for mut team in &mut teams {
        if team.entries.contains(&name.0) && !team.members.contains(&event.entity) {
            team.members.insert(event.entity);
        }
    }
}

fn team_arg(ctx: &CommandContext) -> Entity {
    *ctx.get::<Entity>("team")
        .expect("team is a required argument")
}

fn label(team: &Team) -> String {
    format!("[{}]", team.display_name)
}

fn team_label(world: &World, team: Entity) -> String {
    world.get::<Team>(team).map(label).unwrap_or_default()
}

fn players_named(world: &mut World, holder: &str) -> Vec<Entity> {
    world
        .query::<(Entity, &PlayerName)>()
        .iter(world)
        .filter(|(_, name)| name.0 == holder)
        .map(|(entity, _)| entity)
        .collect()
}

fn members(world: &World, team: &Team) -> BTreeSet<String> {
    let mut members = team.entries.clone();
    members.extend(
        team.members
            .iter()
            .filter_map(|member| world.get::<PlayerName>(*member))
            .map(|name| name.0.clone()),
    );
    members
}

/// Takes `holder` off every team, whether it joined by name or by entity.
fn leave_all(world: &mut World, holder: &str) -> bool {
    let players = players_named(world, holder);
    let mut left = false;
    for mut team in world.query::<&mut Team>().iter_mut(world) {
        let listed = team.entries.contains(holder)
            || players.iter().any(|player| team.members.contains(player));
        if listed {
            team.entries.remove(holder);
            for player in &players {
                team.members.remove(player);
            }
            left = true;
        }
    }
    left
}

fn add(ctx: &mut CommandContext) {
    let name = ctx.get::<String>("team").cloned().unwrap_or_default();
    let display = ctx.get::<StyledText>("displayName").map(|t| t.text.clone());
    let outcome = ctx.with_world_mut(|world| {
        if find_team(world, &name).is_some() {
            return Err("A team already exists by that name".to_string());
        }
        let team = Team::new(&name).display_name(display.unwrap_or_else(|| name.clone()));
        let message = format!("Created team {}", label(&team));
        world.spawn((team, CommandTeam));
        Ok(message)
    });
    respond(ctx, outcome);
}

fn remove(ctx: &mut CommandContext) {
    let team = team_arg(ctx);
    let outcome = ctx.with_world_mut(|world| {
        let message = format!("Removed team {}", team_label(world, team));
        if world.get::<CommandTeam>(team).is_some() {
            world.despawn(team);
        } else {
            world.entity_mut(team).remove::<Team>();
        }
        Ok(message)
    });
    respond(ctx, outcome);
}

fn empty(ctx: &mut CommandContext) {
    let team = team_arg(ctx);
    let outcome = ctx.with_world_mut(|world| {
        let count = world
            .get::<Team>(team)
            .map_or(0, |team| members(world, team).len());
        if count == 0 {
            return Err("Nothing changed. That team is already empty".to_string());
        }
        let mut team = world.get_mut::<Team>(team).unwrap();
        team.members.clear();
        team.entries.clear();
        Ok(format!(
            "Removed {count} member(s) from team {}",
            label(&team)
        ))
    });
    respond(ctx, outcome);
}

fn join(ctx: &mut CommandContext) {
    let team = team_arg(ctx);
    let holders = match ctx.get::<Vec<String>>("members") {
        Some(holders) => holders.clone(),
        None => match ctx.player_name() {
            Some(name) => vec![name],
            None => return ctx.reply_error("A player is required to run this command here"),
        },
    };
    let outcome = ctx.with_world_mut(|world| {
        for holder in &holders {
            leave_all(world, holder);
            let online = players_named(world, holder);
            let mut team = world.get_mut::<Team>(team).unwrap();
            team.add_entry(holder.clone());
            team.members.extend(online);
        }
        let team = team_label(world, team);
        Ok(match holders.as_slice() {
            [single] => format!("Added {single} to team {team}"),
            many => format!("Added {} members to team {team}", many.len()),
        })
    });
    respond(ctx, outcome);
}

fn leave(ctx: &mut CommandContext) {
    let holders = ctx
        .get::<Vec<String>>("members")
        .cloned()
        .unwrap_or_default();
    let outcome = ctx.with_world_mut(|world| {
        for holder in &holders {
            leave_all(world, holder);
        }
        Ok(match holders.as_slice() {
            [single] => format!("Removed {single} from any team"),
            many => format!("Removed {} members from any team", many.len()),
        })
    });
    respond(ctx, outcome);
}

fn list(ctx: &mut CommandContext) {
    let team = ctx.get::<Entity>("team").copied();
    let outcome = ctx.with_world(|world| match team {
        Some(team) => {
            let team = world.get::<Team>(team).unwrap();
            let members = members(world, team);
            if members.is_empty() {
                Ok(format!("There are no members on team {}", label(team)))
            } else {
                Ok(format!(
                    "Team {} has {} member(s): {}",
                    label(team),
                    members.len(),
                    members.into_iter().collect::<Vec<_>>().join(", ")
                ))
            }
        }
        None => {
            let mut teams: Vec<&Team> = world
                .try_query::<&Team>()
                .map(|mut query| query.iter(world).collect())
                .unwrap_or_default();
            teams.sort_by(|a, b| a.name.cmp(&b.name));
            if teams.is_empty() {
                Ok("There are no teams".to_string())
            } else {
                Ok(format!(
                    "There are {} team(s): {}",
                    teams.len(),
                    teams.into_iter().map(label).collect::<Vec<_>>().join(", ")
                ))
            }
        }
    });
    respond(ctx, outcome);
}

/// Applies `change` unless `unchanged` already holds, as vanilla does.
fn modify(
    ctx: &mut CommandContext,
    unchanged: impl FnOnce(&Team) -> Option<&'static str>,
    change: impl FnOnce(&mut Team) -> String,
) {
    let team = team_arg(ctx);
    let outcome = ctx.with_world_mut(|world| {
        let mut team = world.get_mut::<Team>(team).unwrap();
        if let Some(error) = unchanged(&team) {
            return Err(error.to_string());
        }
        Ok(change(&mut team))
    });
    respond(ctx, outcome);
}

fn text_arg(ctx: &CommandContext, name: &str) -> String {
    ctx.get::<StyledText>(name)
        .map(|text| text.text.clone())
        .unwrap_or_default()
}

fn modify_display_name(ctx: &mut CommandContext) {
    let name = text_arg(ctx, "displayName");
    modify(
        ctx,
        |team| {
            (team.display_name == name)
                .then_some("Nothing changed. That team already has that name")
        },
        |team| {
            team.display_name = name.clone();
            format!("Updated the name of team {}", label(team))
        },
    );
}

fn team_color(name: &str) -> TeamColor {
    TextColor::parse(name).map_or(TeamColor::Reset, TeamColor::from)
}

fn modify_color(ctx: &mut CommandContext) {
    let name = ctx.get::<String>("value").cloned().unwrap_or_default();
    let color = team_color(&name);
    modify(
        ctx,
        |team| (team.color == color).then_some("Nothing changed. That team already has that color"),
        |team| {
            team.color = color;
            format!("Updated the color for team {} to {name}", label(team))
        },
    );
}

fn modify_friendly_fire(ctx: &mut CommandContext) {
    let allowed = *ctx.get::<bool>("allowed").unwrap();
    modify(
        ctx,
        |team| {
            (team.friendly_fire == allowed).then_some(if allowed {
                "Nothing changed. Friendly fire is already enabled for that team"
            } else {
                "Nothing changed. Friendly fire is already disabled for that team"
            })
        },
        |team| {
            team.friendly_fire = allowed;
            let verb = if allowed { "Enabled" } else { "Disabled" };
            format!("{verb} friendly fire for team {}", label(team))
        },
    );
}

fn modify_see_invisibles(ctx: &mut CommandContext) {
    let visible = *ctx.get::<bool>("allowed").unwrap();
    modify(
        ctx,
        |team| {
            (team.see_invisible_friends == visible).then_some(if visible {
                "Nothing changed. That team can already see invisible teammates"
            } else {
                "Nothing changed. That team already can't see invisible teammates"
            })
        },
        |team| {
            team.see_invisible_friends = visible;
            if visible {
                format!("Team {} can now see invisible teammates", label(team))
            } else {
                format!("Team {} can no longer see invisible teammates", label(team))
            }
        },
    );
}

fn set_name_tags(
    world: &mut World,
    team: Entity,
    name: &'static str,
    visibility: NameTagVisibility,
) -> Outcome {
    let mut team = world.get_mut::<Team>(team).unwrap();
    if team.name_tags == visibility {
        return Err("Nothing changed. Nametag visibility is already that value".into());
    }
    team.name_tags = visibility;
    Ok(format!(
        "Nametag visibility for team {} is now \"{name}\"",
        label(&team)
    ))
}

fn set_death_messages(
    world: &mut World,
    team: Entity,
    name: &'static str,
    visibility: NameTagVisibility,
) -> Outcome {
    let current = world
        .get::<DeathMessageVisibility>(team)
        .copied()
        .unwrap_or_default();
    if current.0 == visibility {
        return Err("Nothing changed. Death message visibility is already that value".into());
    }
    world
        .entity_mut(team)
        .insert(DeathMessageVisibility(visibility));
    Ok(format!(
        "Death message visibility for team {} is now \"{name}\"",
        team_label(world, team)
    ))
}

fn set_collision(
    world: &mut World,
    team: Entity,
    name: &'static str,
    rule: CollisionRule,
) -> Outcome {
    let mut team = world.get_mut::<Team>(team).unwrap();
    if team.collision == rule {
        return Err("Nothing changed. Collision rule is already that value".into());
    }
    team.collision = rule;
    Ok(format!(
        "Collision rule for team {} is now \"{name}\"",
        label(&team)
    ))
}

fn modify_prefix(ctx: &mut CommandContext) {
    let prefix = text_arg(ctx, "prefix");
    modify(
        ctx,
        |_| None,
        |team| {
            team.prefix = prefix.clone();
            format!("Team prefix set to {prefix}")
        },
    );
}

fn modify_suffix(ctx: &mut CommandContext) {
    let suffix = text_arg(ctx, "suffix");
    modify(
        ctx,
        |_| None,
        |team| {
            team.suffix = suffix.clone();
            format!("Team suffix set to {suffix}")
        },
    );
}

#[cfg(test)]
mod tests {
    use voidmc_protocol::clientbound::{PlayPacket, TeamAction};

    use super::*;
    use crate::Access;
    use crate::testing::Harness;

    fn team_packets(run: &crate::testing::Run) -> Vec<(u32, String, &'static str, Vec<String>)> {
        let mut packets: Vec<_> = run
            .packets
            .iter()
            .filter_map(|(client, packet)| match packet {
                PlayPacket::SetPlayerTeam(p) => Some(match &p.action {
                    TeamAction::Create { entities, .. } => {
                        (*client, p.name.clone(), "create", entities.clone())
                    }
                    TeamAction::Remove => (*client, p.name.clone(), "remove", vec![]),
                    TeamAction::Update(_) => (*client, p.name.clone(), "update", vec![]),
                    TeamAction::AddEntities(e) => (*client, p.name.clone(), "join", e.clone()),
                    TeamAction::RemoveEntities(e) => (*client, p.name.clone(), "leave", e.clone()),
                }),
                _ => None,
            })
            .collect();
        packets.sort();
        packets
    }

    #[test]
    fn add_creates_a_team_on_every_client() {
        let mut h = Harness::new();
        let run = h.ok(r#"team add red {"text":"Red Team","color":"red"}"#);
        assert_eq!(run.reply(), "Created team [Red Team]");
        assert_eq!(
            team_packets(&run),
            vec![
                (1, "red".into(), "create", vec![]),
                (2, "red".into(), "create", vec![]),
            ]
        );
        assert_eq!(h.team("red").unwrap().display_name, "Red Team");

        assert_eq!(
            h.run("team add red").reply(),
            "A team already exists by that name"
        );
        assert_eq!(h.ok("team add blue").reply(), "Created team [blue]");
        assert_eq!(
            h.run("team add bad#name").replies,
            vec![
                "Invalid value 'bad#name' for <team>: expected name ('bad#name' may only contain letters, digits and _ - . +)",
                "Usage: /team add <team:name> [displayName:component]...",
            ]
        );
    }

    #[test]
    fn join_leave_and_empty_track_members_by_name() {
        let mut h = Harness::new();
        h.ok("team add red");
        h.ok("team add blue");

        let run = h.ok("team join red");
        assert_eq!(run.reply(), "Added Alice to team [red]");
        assert!(h.team("red").unwrap().contains(h.alice));
        assert_eq!(
            team_packets(&run),
            vec![
                (1, "red".into(), "join", vec!["Alice".into()]),
                (2, "red".into(), "join", vec!["Alice".into()]),
            ]
        );

        let run = h.ok("team join blue @a");
        assert_eq!(run.reply(), "Added 2 members to team [blue]");
        let packets = team_packets(&run);
        assert!(packets.contains(&(1, "red".into(), "leave", vec!["Alice".into()])));
        assert!(packets.contains(&(1, "blue".into(), "join", vec!["Alice".into(), "Bob".into()])));
        assert!(h.team("red").unwrap().entries.is_empty());
        assert!(!h.team("red").unwrap().contains(h.alice));
        assert!(h.team("blue").unwrap().contains(h.bob));

        assert_eq!(
            h.ok("team join red Offline").reply(),
            "Added Offline to team [red]"
        );
        assert_eq!(
            h.ok("team list red").reply(),
            "Team [red] has 1 member(s): Offline"
        );
        assert_eq!(h.ok("team leave Bob").reply(), "Removed Bob from any team");
        assert_eq!(
            h.ok("team list blue").reply(),
            "Team [blue] has 1 member(s): Alice"
        );

        assert_eq!(
            h.ok("team empty blue").reply(),
            "Removed 1 member(s) from team [blue]"
        );
        assert_eq!(
            h.run("team empty blue").reply(),
            "Nothing changed. That team is already empty"
        );
        assert_eq!(
            h.ok("team list blue").reply(),
            "There are no members on team [blue]"
        );
    }

    #[test]
    fn players_are_back_on_their_team_after_logging_in() {
        use voidmc::components::ClientId;

        let mut h = Harness::new();
        h.ok("team add red");
        h.ok("team join red Carol");
        h.ok("team join red");
        let carol = h
            .app
            .world_mut()
            .spawn((ClientId(3), PlayerName("Carol".into()), PlayerReady))
            .id();
        h.app.update();
        assert!(h.team("red").unwrap().contains(carol));

        let alice = h.alice;
        h.app.world_mut().despawn(alice);
        h.app.update();
        assert!(!h.team("red").unwrap().contains(alice));
        let alice = h
            .app
            .world_mut()
            .spawn((ClientId(1), PlayerName("Alice".into()), PlayerReady))
            .id();
        h.app.update();
        let red = h.team("red").unwrap();
        assert!(red.contains(alice));
        assert_eq!(red.entries.len(), 2);
    }

    #[test]
    fn join_takes_players_off_code_created_teams() {
        let mut h = Harness::new();
        let alice = h.alice;
        h.app.world_mut().spawn(Team::new("code").members([alice]));
        h.app.update();
        h.ok("team add red");
        h.ok("team join red");
        assert!(h.team("code").unwrap().members.is_empty());
        assert_eq!(
            h.ok("team list red").reply(),
            "Team [red] has 1 member(s): Alice"
        );
    }

    #[test]
    fn removing_a_code_team_keeps_its_entity() {
        #[derive(Component)]
        struct Arena;
        let mut h = Harness::new();
        let arena = h.app.world_mut().spawn((Team::new("code"), Arena)).id();
        h.app.update();
        h.ok("team remove code");
        assert!(h.app.world().get::<Arena>(arena).is_some());
        assert!(h.team("code").is_none());
    }

    #[test]
    fn list_and_remove() {
        let mut h = Harness::new();
        assert_eq!(h.ok("team list").reply(), "There are no teams");
        h.ok("team add red");
        h.ok("team add blue");
        assert_eq!(
            h.ok("team list").reply(),
            "There are 2 team(s): [blue], [red]"
        );
        let run = h.ok("team remove red");
        assert_eq!(run.reply(), "Removed team [red]");
        assert_eq!(
            team_packets(&run),
            vec![
                (1, "red".into(), "remove", vec![]),
                (2, "red".into(), "remove", vec![]),
            ]
        );
        assert!(h.team("red").is_none());
        assert_eq!(h.run("team remove red").replies.len(), 2);
    }

    #[test]
    fn modify_updates_options_and_reports_no_ops() {
        let mut h = Harness::new();
        h.ok("team add red");

        let run = h.ok("team modify red color red");
        assert_eq!(run.reply(), "Updated the color for team [red] to red");
        assert_eq!(team_packets(&run).len(), 2);
        assert_eq!(h.team("red").unwrap().color, TeamColor::Red);
        assert_eq!(
            h.run("team modify red color red").reply(),
            "Nothing changed. That team already has that color"
        );
        h.ok("team modify red color reset");
        assert_eq!(h.team("red").unwrap().color, TeamColor::Reset);

        h.ok("team modify red friendlyFire false");
        assert_eq!(
            h.run("team modify red friendlyFire false").reply(),
            "Nothing changed. Friendly fire is already disabled for that team"
        );
        h.ok("team modify red seeFriendlyInvisibles false");
        assert_eq!(
            h.ok("team modify red nametagVisibility hideForOtherTeams")
                .reply(),
            "Nametag visibility for team [red] is now \"hideForOtherTeams\""
        );
        h.ok("team modify red collisionRule pushOwnTeam");
        h.ok("team modify red deathMessageVisibility never");
        assert_eq!(
            h.run("team modify red deathMessageVisibility never")
                .reply(),
            "Nothing changed. Death message visibility is already that value"
        );
        assert_eq!(
            h.ok("team modify red prefix \"[R] \"").reply(),
            "Team prefix set to [R] "
        );
        h.ok("team modify red suffix {text:' *'}");
        h.ok("team modify red displayName 'The Reds'");

        let team = h.team("red").unwrap();
        assert!(!team.friendly_fire);
        assert!(!team.see_invisible_friends);
        assert_eq!(team.name_tags, NameTagVisibility::HideForOtherTeams);
        assert_eq!(team.collision, CollisionRule::PushOwnTeam);
        assert_eq!(team.prefix, "[R] ");
        assert_eq!(team.suffix, " *");
        assert_eq!(team.display_name, "The Reds");
        let world = h.app.world_mut();
        let death = world
            .query::<&DeathMessageVisibility>()
            .single(world)
            .unwrap();
        assert_eq!(death.0, NameTagVisibility::Never);
    }

    #[test]
    fn unknown_teams_and_options_are_errors() {
        let mut h = Harness::new();
        h.ok("team add red");
        for line in [
            "team modify green color red",
            "team modify red paint red",
            "team modify red nametagVisibility sometimes",
            "team join green",
        ] {
            let run = h.run(line);
            assert_eq!(run.replies.len(), 2, "{line}: {:?}", run.replies);
            assert!(run.packets.is_empty());
        }
    }

    #[test]
    fn non_operators_are_refused_unless_everyone_is_allowed() {
        let mut h = Harness::new();
        let bob = h.bob;
        let run = h.run_as(bob, "team add red");
        assert_eq!(
            run.reply(),
            "You do not have permission to use this command"
        );
        assert!(h.team("red").is_none());

        let mut h = Harness::with_access(Access::Everyone);
        let bob = h.bob;
        assert_eq!(h.run_as(bob, "team add red").reply(), "Created team [red]");
    }
}
