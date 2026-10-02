//! Vanilla commands as an opt-in plugin: `/team` drives the engine's
//! [`voidmc::Team`] entities, `/scoreboard` drives a vanilla [`Scoreboard`]
//! whose displayed objectives are mirrored into [`voidmc::Objective`]s.

mod args;
mod model;
mod scoreboard;
mod team;
mod text;

use bevy_app::{App, Plugin, PostUpdate};
use bevy_ecs::prelude::*;
use voidmc::components::Operator;
use voidmc::{CommandContext, CommandRegistry, VoidSystems};

pub use args::{
    ComponentArg, CriteriaArg, NameArg, ObjectiveArg, Operation, OperationArg, ScoreHolderArg,
    SlotArg, StyleArg, TeamArg,
};
pub use model::{Criteria, DisplayedObjective, ScoreObjective, Scoreboard};
pub use scoreboard::scoreboard_command;
pub use team::{DeathMessageVisibility, team_command};
pub use text::{StyledText, parse_component, parse_style};

/// Who may run the commands. Vanilla restricts both to operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Access {
    #[default]
    Operators,
    Everyone,
}

impl Access {
    pub fn allows(self, world: &World, player: Entity) -> bool {
        self == Access::Everyone || world.get::<Operator>(player).is_some()
    }
}

fn respond(ctx: &CommandContext, result: Result<String, String>) {
    match result {
        Ok(message) => ctx.reply(&message),
        Err(error) => ctx.reply_error(&error),
    }
}

fn register(app: &mut App, command: voidmc::Command) {
    app.init_resource::<CommandRegistry>();
    app.world_mut()
        .resource_mut::<CommandRegistry>()
        .register(command);
}

/// Registers `/team`.
#[derive(Debug, Clone, Copy, Default)]
pub struct TeamCommandsPlugin {
    pub access: Access,
}

impl Plugin for TeamCommandsPlugin {
    fn build(&self, app: &mut App) {
        register(app, team_command(self.access));
    }
}

/// Registers `/scoreboard` and the [`Scoreboard`] resource it edits.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScoreboardCommandsPlugin {
    pub access: Access,
}

impl Plugin for ScoreboardCommandsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Scoreboard>().add_systems(
            PostUpdate,
            model::sync_displays
                .run_if(resource_changed::<Scoreboard>)
                .before(VoidSystems::ScoreboardSync),
        );
        register(app, scoreboard_command(self.access));
    }
}

/// Every vanilla command of this crate.
#[derive(Debug, Clone, Copy, Default)]
pub struct VanillaCommandsPlugin {
    pub access: Access,
}

impl Plugin for VanillaCommandsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            TeamCommandsPlugin {
                access: self.access,
            },
            ScoreboardCommandsPlugin {
                access: self.access,
            },
        ));
    }
}

#[cfg(test)]
mod testing;

#[cfg(test)]
mod tests {
    use voidmc_codec::Encode;
    use voidmc_protocol::clientbound::commands::{CommandNode, Commands, Parser};

    use super::*;
    use crate::testing::Harness;

    fn complete(h: &Harness, line: &str) -> Vec<String> {
        let world = h.app.world();
        world
            .resource::<CommandRegistry>()
            .complete(line, world, h.alice)
            .map(|completion| completion.matches)
            .unwrap_or_default()
    }

    #[test]
    fn dashes_and_double_dashes_are_plain_text() {
        let mut h = Harness::new();
        assert_eq!(
            h.ok(r#"team add red "Red -big team""#).reply(),
            "Created team [Red -big team]"
        );
        assert_eq!(h.ok("team add -blue").reply(), "Created team [-blue]");
        assert_eq!(h.ok("team add --").reply(), "Created team [--]");
        h.ok("team add green");
        assert_eq!(
            h.ok(r#"team modify green prefix {text:"a --b"}"#).reply(),
            "Team prefix set to a --b"
        );
        h.ok("scoreboard objectives add k dummy");
        assert_eq!(
            h.ok("scoreboard players set -abc k 1").reply(),
            "Set [k] for -abc to 1"
        );
    }

    #[test]
    fn non_operators_learn_nothing_from_errors_or_completion() {
        let mut h = Harness::new();
        h.ok("team add red");
        let bob = h.bob;
        let denied = "You do not have permission to use this command";
        for line in [
            "team modify nope color red",
            "team list red",
            "scoreboard players get x nope",
        ] {
            assert_eq!(h.run_as(bob, line).replies, vec![denied], "{line}");
        }
        let world = h.app.world();
        let completion = world
            .resource::<CommandRegistry>()
            .complete("/team join ", world, bob)
            .unwrap();
        assert!(completion.matches.is_empty());
    }

    #[test]
    fn tab_completion_reaches_teams_objectives_and_holders() {
        let mut h = Harness::new();
        h.ok("team add red");
        h.ok("team add blue");
        h.ok("scoreboard objectives add kills dummy");

        assert_eq!(complete(&h, "/team join r"), vec!["red"]);
        assert_eq!(complete(&h, "/team join red B"), vec!["Bob"]);
        assert_eq!(
            complete(&h, "/team modify blue n"),
            vec!["nametagVisibility"]
        );
        assert_eq!(
            complete(&h, "/team modify blue collisionRule push"),
            vec!["pushOtherTeams", "pushOwnTeam"]
        );
        assert_eq!(complete(&h, "/scoreboard p"), vec!["players"]);
        assert_eq!(complete(&h, "/scoreboard players set @a k"), vec!["kills"]);
        assert_eq!(
            complete(&h, "/scoreboard players operation * kills = Alice "),
            vec!["kills"]
        );
        assert!(complete(&h, "/scoreboard players set * kills ").is_empty());
    }

    fn find<'a>(tree: &'a Commands, path: &[&str]) -> &'a CommandNode {
        let mut index = tree.root_index;
        for name in path {
            index = *tree.nodes[index as usize]
                .children
                .iter()
                .find(|child| tree.nodes[**child as usize].name.as_deref() == Some(*name))
                .unwrap_or_else(|| panic!("missing {name} in {path:?}"));
        }
        &tree.nodes[index as usize]
    }

    #[test]
    fn command_tree_matches_the_vanilla_shape() {
        let h = Harness::new();
        let tree = h
            .app
            .world()
            .resource::<CommandRegistry>()
            .build_command_tree();

        let holders = find(&tree, &["scoreboard", "players", "operation", "targets"]);
        assert_eq!(holders.parser, Some(Parser::ScoreHolder { multiple: true }));
        assert_eq!(
            holders.suggestions_type.as_deref(),
            Some("minecraft:ask_server")
        );
        let source = find(
            &tree,
            &[
                "scoreboard",
                "players",
                "operation",
                "targets",
                "targetObjective",
                "operation",
                "source",
                "sourceObjective",
            ],
        );
        assert!(source.is_executable);
        assert_eq!(source.parser, Some(Parser::Objective));

        let slot = find(&tree, &["scoreboard", "objectives", "setdisplay", "slot"]);
        assert_eq!(slot.parser, Some(Parser::ScoreboardSlot));
        assert!(slot.is_executable);

        let format = find(
            &tree,
            &[
                "scoreboard",
                "players",
                "display",
                "numberformat",
                "targets",
                "objective",
            ],
        );
        assert!(format.is_executable);
        for branch in ["blank", "fixed", "styled"] {
            find(
                &tree,
                &[
                    "scoreboard",
                    "players",
                    "display",
                    "numberformat",
                    "targets",
                    "objective",
                    branch,
                ],
            );
        }
        let prefix = find(&tree, &["team", "modify", "team", "prefix", "prefix"]);
        assert_eq!(prefix.parser, Some(Parser::Component));
        let team = find(&tree, &["team", "modify", "team"]);
        assert_eq!(team.parser, Some(Parser::Team));
        assert!(!team.is_executable);
        assert!(
            find(
                &tree,
                &[
                    "team",
                    "modify",
                    "team",
                    "nametagVisibility",
                    "hideForOwnTeam"
                ]
            )
            .is_executable
        );
        assert!(find(&tree, &["team", "join", "team"]).is_executable);

        let mut names: Vec<_> = tree.nodes[tree.root_index as usize]
            .children
            .iter()
            .map(|child| tree.nodes[*child as usize].name.clone().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, vec!["scoreboard", "team"]);
        for node in &tree.nodes {
            for index in node.children.iter().chain(&node.redirect_node) {
                assert!((*index as usize) < tree.nodes.len(), "{node:?}");
            }
            assert_eq!(node.parser.is_some(), node.node_type == 2, "{node:?}");
        }

        let mut buf = Vec::new();
        tree.encode(&mut buf);
        let mut count = Vec::new();
        voidmc_codec::VarI32(tree.nodes.len() as i32).encode(&mut count);
        assert_eq!(&buf[..count.len()], count.as_slice());
    }
}
