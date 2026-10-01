use std::any::Any;
use std::sync::Arc;

use bevy_ecs::prelude::*;
use voidmc::components::{PlayerName, PlayerReady};
use voidmc::{ArgParser, DisplaySlot, ParseContext, PlayerSelector, ScoreFormat, Team};
use voidmc_protocol::clientbound::commands::{Parser, StringType};

use crate::model::{Criteria, Scoreboard};
use crate::text::{StyledText, parse_component, parse_style};

const ASK_SERVER: &str = "minecraft:ask_server";

type Parsed = Result<Box<dyn Any + Send + Sync>, String>;

fn boxed<T: Send + Sync + 'static>(value: T) -> Parsed {
    Ok(Box::new(value))
}

fn completions<'a>(candidates: impl IntoIterator<Item = &'a str>, partial: &str) -> Vec<String> {
    candidates
        .into_iter()
        .filter(|candidate| {
            candidate
                .get(..partial.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(partial))
        })
        .map(str::to_string)
        .collect()
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "_-.+".contains(c)
}

/// A new team or objective name: one word of `0-9 A-Z a-z _ - . +`.
pub struct NameArg;

impl ArgParser for NameArg {
    fn type_name(&self) -> &str {
        "name"
    }

    fn parse(&self, input: &str) -> Parsed {
        if input.is_empty() || !input.chars().all(is_name_char) {
            return Err(format!(
                "'{input}' may only contain letters, digits and _ - . +"
            ));
        }
        boxed(input.to_string())
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::String(StringType::SingleWord))
    }
}

pub(crate) fn find_team(world: &World, name: &str) -> Option<Entity> {
    let mut teams = world.try_query::<(Entity, &Team)>()?;
    teams
        .iter(world)
        .find(|(_, team)| team.name == name)
        .map(|(entity, _)| entity)
}

/// An existing team, resolved to its entity.
pub struct TeamArg;

impl ArgParser for TeamArg {
    fn type_name(&self) -> &str {
        "team"
    }

    fn parse(&self, _input: &str) -> Parsed {
        Err("teams need the world to resolve".into())
    }

    fn parse_in(&self, input: &str, ctx: &ParseContext<'_>) -> Parsed {
        find_team(ctx.world, input)
            .map(Box::new)
            .map(|team| team as Box<dyn Any + Send + Sync>)
            .ok_or_else(|| format!("Unknown team '{input}'"))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::Team)
    }

    fn suggestions(&self, partial: &str, world: &World) -> Vec<String> {
        let Some(mut teams) = world.try_query::<&Team>() else {
            return Vec::new();
        };
        let names: Vec<&str> = teams.iter(world).map(|team| team.name.as_str()).collect();
        completions(names, partial)
    }

    fn suggestions_type(&self) -> Option<&str> {
        Some(ASK_SERVER)
    }
}

/// An existing objective, resolved to its name.
pub struct ObjectiveArg;

impl ArgParser for ObjectiveArg {
    fn type_name(&self) -> &str {
        "objective"
    }

    fn parse(&self, _input: &str) -> Parsed {
        Err("objectives need the world to resolve".into())
    }

    fn parse_in(&self, input: &str, ctx: &ParseContext<'_>) -> Parsed {
        ctx.world
            .get_resource::<Scoreboard>()
            .and_then(|board| board.objective(input))
            .map(|_| Box::new(input.to_string()) as Box<dyn Any + Send + Sync>)
            .ok_or_else(|| format!("Unknown scoreboard objective '{input}'"))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::Objective)
    }

    fn suggestions(&self, partial: &str, world: &World) -> Vec<String> {
        world
            .get_resource::<Scoreboard>()
            .map(|board| completions(board.objectives().map(|(name, _)| name), partial))
            .unwrap_or_default()
    }

    fn suggestions_type(&self) -> Option<&str> {
        Some(ASK_SERVER)
    }
}

pub struct CriteriaArg;

impl ArgParser for CriteriaArg {
    fn type_name(&self) -> &str {
        "criteria"
    }

    fn parse(&self, input: &str) -> Parsed {
        match input {
            "dummy" => boxed(Criteria::Dummy),
            "trigger" => boxed(Criteria::Trigger),
            _ => Err(format!(
                "criterion '{input}' is not supported (this server tracks dummy and trigger)"
            )),
        }
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::ObjectiveCriteria)
    }
}

/// A `scoreboard players operation` operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Assign,
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Min,
    Max,
    Swap,
}

impl Operation {
    const ALL: [(&'static str, Operation); 9] = [
        ("=", Operation::Assign),
        ("+=", Operation::Add),
        ("-=", Operation::Subtract),
        ("*=", Operation::Multiply),
        ("/=", Operation::Divide),
        ("%=", Operation::Modulo),
        ("<", Operation::Min),
        (">", Operation::Max),
        ("><", Operation::Swap),
    ];

    /// Returns the new `(target, source)`; Java integer semantics (wrapping,
    /// floored division and modulo).
    pub fn apply(self, target: i32, source: i32) -> Result<(i32, i32), String> {
        let floor_div = |a: i32, b: i32| {
            let quotient = a.wrapping_div(b);
            if a.wrapping_rem(b) != 0 && ((a < 0) != (b < 0)) {
                quotient.wrapping_sub(1)
            } else {
                quotient
            }
        };
        let floor_mod = |a: i32, b: i32| {
            let remainder = a.wrapping_rem(b);
            if remainder != 0 && ((remainder < 0) != (b < 0)) {
                remainder + b
            } else {
                remainder
            }
        };
        if matches!(self, Operation::Divide | Operation::Modulo) && source == 0 {
            return Err("Can't divide by zero".into());
        }
        Ok(match self {
            Operation::Assign => (source, source),
            Operation::Add => (target.wrapping_add(source), source),
            Operation::Subtract => (target.wrapping_sub(source), source),
            Operation::Multiply => (target.wrapping_mul(source), source),
            Operation::Divide => (floor_div(target, source), source),
            Operation::Modulo => (floor_mod(target, source), source),
            Operation::Min => (target.min(source), source),
            Operation::Max => (target.max(source), source),
            Operation::Swap => (source, target),
        })
    }
}

pub struct OperationArg;

impl ArgParser for OperationArg {
    fn type_name(&self) -> &str {
        "operation"
    }

    fn parse(&self, input: &str) -> Parsed {
        Operation::ALL
            .into_iter()
            .find(|(symbol, _)| *symbol == input)
            .map(|(_, operation)| Box::new(operation) as Box<dyn Any + Send + Sync>)
            .ok_or_else(|| "Invalid operation".to_string())
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::Operation)
    }
}

const SLOTS: [(&str, DisplaySlot); 19] = [
    ("list", DisplaySlot::List),
    ("sidebar", DisplaySlot::Sidebar),
    ("below_name", DisplaySlot::BelowName),
    ("sidebar.team.black", DisplaySlot::SidebarTeamBlack),
    ("sidebar.team.dark_blue", DisplaySlot::SidebarTeamDarkBlue),
    ("sidebar.team.dark_green", DisplaySlot::SidebarTeamDarkGreen),
    ("sidebar.team.dark_aqua", DisplaySlot::SidebarTeamDarkAqua),
    ("sidebar.team.dark_red", DisplaySlot::SidebarTeamDarkRed),
    (
        "sidebar.team.dark_purple",
        DisplaySlot::SidebarTeamDarkPurple,
    ),
    ("sidebar.team.gold", DisplaySlot::SidebarTeamGold),
    ("sidebar.team.gray", DisplaySlot::SidebarTeamGray),
    ("sidebar.team.dark_gray", DisplaySlot::SidebarTeamDarkGray),
    ("sidebar.team.blue", DisplaySlot::SidebarTeamBlue),
    ("sidebar.team.green", DisplaySlot::SidebarTeamGreen),
    ("sidebar.team.aqua", DisplaySlot::SidebarTeamAqua),
    ("sidebar.team.red", DisplaySlot::SidebarTeamRed),
    (
        "sidebar.team.light_purple",
        DisplaySlot::SidebarTeamLightPurple,
    ),
    ("sidebar.team.yellow", DisplaySlot::SidebarTeamYellow),
    ("sidebar.team.white", DisplaySlot::SidebarTeamWhite),
];

pub(crate) fn slot_name(slot: DisplaySlot) -> &'static str {
    SLOTS
        .into_iter()
        .find(|(_, candidate)| *candidate == slot)
        .map_or("?", |(name, _)| name)
}

pub struct SlotArg;

impl ArgParser for SlotArg {
    fn type_name(&self) -> &str {
        "scoreboard_slot"
    }

    fn parse(&self, input: &str) -> Parsed {
        SLOTS
            .into_iter()
            .find(|(name, _)| *name == input)
            .map(|(_, slot)| Box::new(slot) as Box<dyn Any + Send + Sync>)
            .ok_or_else(|| format!("Unknown display slot '{input}'"))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::ScoreboardSlot)
    }
}

/// Score holders: `*` (every tracked holder), a player selector (`@a`,
/// `@s`, `@p`, `@r`) or any literal name or UUID, online or not. Parses to
/// `Vec<String>`, or to `String` when built with [`ScoreHolderArg::single`].
pub struct ScoreHolderArg {
    multiple: bool,
}

impl ScoreHolderArg {
    pub fn single() -> Arc<Self> {
        Arc::new(Self { multiple: false })
    }

    pub fn multiple() -> Arc<Self> {
        Arc::new(Self { multiple: true })
    }

    fn resolve(&self, input: &str, ctx: &ParseContext<'_>) -> Result<Vec<String>, String> {
        if input == "*" {
            if !self.multiple {
                return Err("'*' selects several score holders, expected one".into());
            }
            let holders: Vec<String> = ctx
                .world
                .get_resource::<Scoreboard>()
                .map(|board| board.holders().into_iter().map(str::to_string).collect())
                .unwrap_or_default();
            if holders.is_empty() {
                return Err("No score holder was found".into());
            }
            return Ok(holders);
        }
        if !input.starts_with('@') {
            return Ok(vec![input.to_string()]);
        }
        let selector = PlayerSelector::parse(input)?;
        if !self.multiple && !selector.is_single() {
            return Err(format!("'{input}' selects several players, expected one"));
        }
        let names: Vec<String> = selector
            .resolve(ctx)?
            .into_iter()
            .filter_map(|player| ctx.world.get::<PlayerName>(player))
            .map(|name| name.0.clone())
            .collect();
        if names.is_empty() {
            return Err("No player was found".into());
        }
        Ok(names)
    }
}

impl ArgParser for ScoreHolderArg {
    fn type_name(&self) -> &str {
        if self.multiple {
            "score_holders"
        } else {
            "score_holder"
        }
    }

    fn parse(&self, _input: &str) -> Parsed {
        Err("score holders need an executor".into())
    }

    fn parse_in(&self, input: &str, ctx: &ParseContext<'_>) -> Parsed {
        let mut holders = self.resolve(input, ctx)?;
        if self.multiple {
            boxed(holders)
        } else {
            boxed(holders.swap_remove(0))
        }
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::ScoreHolder {
            multiple: self.multiple,
        })
    }

    fn suggestions(&self, partial: &str, world: &World) -> Vec<String> {
        let mut candidates = vec!["@s", "@p", "@r"];
        if self.multiple {
            candidates.extend(["@a", "*"]);
        }
        let mut names = Vec::new();
        if let Some(mut players) = world.try_query_filtered::<&PlayerName, With<PlayerReady>>() {
            names.extend(players.iter(world).map(|name| name.0.clone()));
        }
        let mut matches = completions(candidates, partial);
        matches.extend(completions(names.iter().map(String::as_str), partial));
        matches
    }

    fn suggestions_type(&self) -> Option<&str> {
        Some(ASK_SERVER)
    }
}

/// A `minecraft:component` text, parsed to [`StyledText`]. Use it as the
/// last, variadic argument: it consumes the rest of the line.
pub struct ComponentArg;

impl ArgParser for ComponentArg {
    fn type_name(&self) -> &str {
        "component"
    }

    fn parse(&self, input: &str) -> Parsed {
        boxed::<StyledText>(parse_component(input)?)
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::Component)
    }
}

/// A `minecraft:style`, parsed to a [`ScoreFormat::Styled`] number format.
pub struct StyleArg;

impl ArgParser for StyleArg {
    fn type_name(&self) -> &str {
        "style"
    }

    fn parse(&self, input: &str) -> Parsed {
        let color = parse_style(input)?
            .ok_or_else(|| "only the color of a style is supported".to_string())?;
        boxed(ScoreFormat::Styled(color))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::Style)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_as<T: 'static>(parser: &dyn ArgParser, input: &str) -> Result<T, String> {
        parser.parse(input).map(|v| *v.downcast::<T>().unwrap())
    }

    #[test]
    fn operations_follow_java_integer_semantics() {
        let op = |symbol: &str| parse_as::<Operation>(&OperationArg, symbol).unwrap();
        assert_eq!(op("=").apply(1, 5), Ok((5, 5)));
        assert_eq!(op("+=").apply(i32::MAX, 1), Ok((i32::MIN, 1)));
        assert_eq!(op("-=").apply(1, 5), Ok((-4, 5)));
        assert_eq!(op("*=").apply(3, -4), Ok((-12, -4)));
        assert_eq!(op("/=").apply(-7, 2), Ok((-4, 2)));
        assert_eq!(op("/=").apply(7, -2), Ok((-4, -2)));
        assert_eq!(op("/=").apply(i32::MIN, -1), Ok((i32::MIN, -1)));
        assert_eq!(op("%=").apply(-7, 3), Ok((2, 3)));
        assert_eq!(op("%=").apply(7, -3), Ok((-2, -3)));
        assert_eq!(op("<").apply(4, 2), Ok((2, 2)));
        assert_eq!(op(">").apply(4, 2), Ok((4, 2)));
        assert_eq!(op("><").apply(4, 2), Ok((2, 4)));
        assert!(op("/=").apply(4, 0).is_err());
        assert!(op("%=").apply(4, 0).is_err());
        assert!(OperationArg.parse("**").is_err());
    }

    #[test]
    fn slots_use_vanilla_names() {
        assert_eq!(
            parse_as::<DisplaySlot>(&SlotArg, "below_name").unwrap(),
            DisplaySlot::BelowName
        );
        assert_eq!(
            parse_as::<DisplaySlot>(&SlotArg, "sidebar.team.light_purple").unwrap(),
            DisplaySlot::SidebarTeamLightPurple
        );
        assert!(SlotArg.parse("belowName").is_err());
        for (name, slot) in SLOTS {
            assert_eq!(slot_name(slot), name);
        }
    }

    #[test]
    fn names_and_criteria() {
        assert_eq!(
            parse_as::<String>(&NameArg, "Red.Team-1+").unwrap(),
            "Red.Team-1+"
        );
        assert!(NameArg.parse("red#1").is_err());
        assert!(NameArg.parse("").is_err());
        assert_eq!(
            parse_as::<Criteria>(&CriteriaArg, "trigger").unwrap(),
            Criteria::Trigger
        );
        assert!(CriteriaArg.parse("health").is_err());
    }

    #[test]
    fn protocol_parsers_match_vanilla() {
        assert_eq!(TeamArg.protocol_parser(), Some(Parser::Team));
        assert_eq!(ObjectiveArg.protocol_parser(), Some(Parser::Objective));
        assert_eq!(
            CriteriaArg.protocol_parser(),
            Some(Parser::ObjectiveCriteria)
        );
        assert_eq!(OperationArg.protocol_parser(), Some(Parser::Operation));
        assert_eq!(SlotArg.protocol_parser(), Some(Parser::ScoreboardSlot));
        assert_eq!(
            ScoreHolderArg::multiple().protocol_parser(),
            Some(Parser::ScoreHolder { multiple: true })
        );
        assert_eq!(ComponentArg.protocol_parser(), Some(Parser::Component));
        assert_eq!(StyleArg.protocol_parser(), Some(Parser::Style));
    }

    #[test]
    fn score_holders_resolve_selectors_wildcards_and_names() {
        let mut world = World::new();
        let alice = world.spawn((PlayerName("Alice".into()), PlayerReady)).id();
        world.spawn((PlayerName("Bob".into()), PlayerReady));
        let ctx = |world: &World, input: &str, parser: &ScoreHolderArg| {
            parser.resolve(
                input,
                &ParseContext {
                    world,
                    executor: alice,
                },
            )
        };
        let many = ScoreHolderArg { multiple: true };
        let one = ScoreHolderArg { multiple: false };

        assert_eq!(ctx(&world, "@s", &one).unwrap(), vec!["Alice"]);
        let mut all = ctx(&world, "@a", &many).unwrap();
        all.sort();
        assert_eq!(all, vec!["Alice", "Bob"]);
        assert_eq!(ctx(&world, "Offline", &one).unwrap(), vec!["Offline"]);
        assert!(ctx(&world, "@a", &one).is_err());
        assert!(ctx(&world, "@e", &many).is_err());
        assert!(ctx(&world, "*", &many).is_err());
        assert!(ctx(&world, "*", &one).is_err());

        let mut board = Scoreboard::default();
        board.add_objective("kills", Criteria::Dummy);
        board.set("#fake", "kills", 1);
        board.set("Carol", "kills", 1);
        world.insert_resource(board);
        assert_eq!(ctx(&world, "*", &many).unwrap(), vec!["#fake", "Carol"]);

        assert_eq!(many.suggestions("", &world).len(), 7);
        assert_eq!(many.suggestions("b", &world), vec!["Bob"]);
        assert_eq!(one.suggestions("@", &world), vec!["@s", "@p", "@r"]);
    }

    #[test]
    fn team_and_objective_args_resolve_and_complete() {
        let mut world = World::new();
        let red = world.spawn(Team::new("red")).id();
        world.spawn(Team::new("blue"));
        let mut board = Scoreboard::default();
        board.add_objective("kills", Criteria::Dummy);
        world.insert_resource(board);
        let executor = world.spawn_empty().id();
        let ctx = ParseContext {
            world: &world,
            executor,
        };

        let team = TeamArg.parse_in("red", &ctx).unwrap();
        assert_eq!(*team.downcast::<Entity>().unwrap(), red);
        assert_eq!(
            TeamArg.parse_in("green", &ctx).err().unwrap(),
            "Unknown team 'green'"
        );
        assert_eq!(TeamArg.suggestions("b", &world), vec!["blue"]);

        assert!(ObjectiveArg.parse_in("kills", &ctx).is_ok());
        assert!(ObjectiveArg.parse_in("deaths", &ctx).is_err());
        assert_eq!(ObjectiveArg.suggestions("K", &world), vec!["kills"]);
    }

    #[test]
    fn styles_become_styled_number_formats() {
        assert_eq!(
            parse_as::<ScoreFormat>(&StyleArg, "{color:gold}").unwrap(),
            ScoreFormat::Styled(voidmc::TextColor::Gold)
        );
        assert!(StyleArg.parse("{bold:true}").is_err());
    }
}
