use std::any::Any;
use std::sync::Arc;

use bevy_ecs::world::World;
use voidmc::ArgParser;
use voidmc_protocol::clientbound::commands::{Parser, StringType};

use crate::block::VERSION;
use crate::math::Direction;

const ASK_SERVER: &str = "minecraft:ask_server";
const MAX_SUGGESTIONS: usize = 64;

/// Pattern or mask text (`stone`, `50%stone,dirt`, `!air`), parsed by the
/// command, with block-name completion on the entry being typed. Sent to the
/// client as a greedy phrase: Brigadier's single word rejects `:[]%,!#`, so
/// it must be a command's last argument.
pub(crate) struct BlocksArg;

impl BlocksArg {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl ArgParser for BlocksArg {
    fn type_name(&self) -> &str {
        "blocks"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        Ok(Box::new(input.to_string()))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::String(StringType::GreedyPhrase))
    }

    fn suggestions(&self, partial: &str, _world: &World) -> Vec<String> {
        let split = partial.rfind([',', '%', '!']).map_or(0, |index| index + 1);
        let (head, entry) = partial.split_at(split);
        if entry.contains('[') {
            return Vec::new();
        }
        let typed = entry.strip_prefix("minecraft:").unwrap_or(entry);
        voidmc_data::block_types(VERSION)
            .iter()
            .map(|block| block.name.strip_prefix("minecraft:").unwrap_or(block.name))
            .filter(|name| name.starts_with(typed))
            .take(MAX_SUGGESTIONS)
            .map(|name| format!("{head}{name}"))
            .collect()
    }

    fn suggestions_type(&self) -> Option<&str> {
        Some(ASK_SERVER)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DirectionChoice {
    Me,
    Direction(Direction),
}

impl DirectionChoice {
    pub(crate) fn resolve(self, facing: Direction) -> Direction {
        match self {
            DirectionChoice::Me => facing,
            DirectionChoice::Direction(direction) => direction,
        }
    }
}

const DIRECTION_NAMES: [&str; 7] = ["me", "north", "south", "east", "west", "up", "down"];

pub(crate) struct DirectionArg;

impl DirectionArg {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl ArgParser for DirectionArg {
    fn type_name(&self) -> &str {
        "direction"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        let input = input.to_ascii_lowercase();
        let choice = match input.as_str() {
            "me" | "forward" => DirectionChoice::Me,
            "n" => DirectionChoice::Direction(Direction::North),
            "s" => DirectionChoice::Direction(Direction::South),
            "e" => DirectionChoice::Direction(Direction::East),
            "w" => DirectionChoice::Direction(Direction::West),
            "u" => DirectionChoice::Direction(Direction::Up),
            "d" => DirectionChoice::Direction(Direction::Down),
            name => Direction::from_name(name)
                .map(DirectionChoice::Direction)
                .ok_or_else(|| format!("'{name}' is not a direction"))?,
        };
        Ok(Box::new(choice))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::String(StringType::SingleWord))
    }

    fn suggestions(&self, partial: &str, _world: &World) -> Vec<String> {
        DIRECTION_NAMES
            .into_iter()
            .filter(|name| name.starts_with(&partial.to_ascii_lowercase()))
            .map(str::to_string)
            .collect()
    }

    fn suggestions_type(&self) -> Option<&str> {
        Some(ASK_SERVER)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse<T: 'static + Copy>(parser: &dyn ArgParser, input: &str) -> Result<T, String> {
        parser
            .parse(input)
            .map(|value| *value.downcast::<T>().unwrap())
    }

    #[test]
    fn block_suggestions_complete_the_entry_being_typed() {
        let world = World::new();
        let arg = BlocksArg;
        let suggestions = arg.suggestions("50%stone,oak_sta", &world);
        assert!(suggestions.contains(&"50%stone,oak_stairs".to_string()));
        assert!(
            suggestions
                .iter()
                .all(|s| s.starts_with("50%stone,oak_sta"))
        );
        assert!(
            arg.suggestions("!glas", &world)
                .contains(&"!glass".to_string())
        );
        assert!(arg.suggestions("oak_stairs[fa", &world).is_empty());
        assert!(arg.suggestions("", &world).len() <= MAX_SUGGESTIONS);
    }

    #[test]
    fn directions_parse_names_and_aliases() {
        let plain = DirectionArg::new();
        assert_eq!(
            parse::<DirectionChoice>(&*plain, "me"),
            Ok(DirectionChoice::Me)
        );
        assert_eq!(
            parse::<DirectionChoice>(&*plain, "Up"),
            Ok(DirectionChoice::Direction(Direction::Up))
        );
        assert_eq!(
            parse::<DirectionChoice>(&*plain, "w"),
            Ok(DirectionChoice::Direction(Direction::West))
        );
        assert!(parse::<DirectionChoice>(&*plain, "sideways").is_err());
        assert_eq!(
            plain.suggestions("so", &World::new()),
            vec!["south".to_string()]
        );
    }
}
