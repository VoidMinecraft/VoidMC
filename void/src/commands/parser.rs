use std::any::Any;
use std::sync::Arc;

use bevy_ecs::prelude::Entity;
use bevy_ecs::world::World;
use uuid::Uuid;
use voidmc_protocol::clientbound::commands::{Parser, StringType};

use crate::components::{Position, Rotation};
use crate::item::ItemId;
use crate::world::DimensionId;

pub use super::coordinates::{BlockPosArg, Vec3Arg};
pub use super::selector::{PlayerArg, PlayerSelector, PlayersArg};

/// Who runs the command, for parsers whose value depends on the executor
/// (relative coordinates, `@s`, `@p`, ...).
pub struct ParseContext<'w> {
    pub world: &'w World,
    pub executor: Entity,
}

impl ParseContext<'_> {
    pub fn position(&self) -> Option<Position> {
        self.world.get::<Position>(self.executor).copied()
    }

    pub fn rotation(&self) -> Option<Rotation> {
        self.world.get::<Rotation>(self.executor).copied()
    }
}

/// Trait for argument parsers that both map to the Minecraft protocol
/// command tree (for tab-completion) and actually parse/validate values.
pub trait ArgParser: Send + Sync {
    /// Human-readable type name for error messages (e.g., "integer", "player").
    fn type_name(&self) -> &str;

    /// Parse a string token into a typed value without executor context.
    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String>;

    /// Parse with access to the executor; the default ignores the context.
    fn parse_in(
        &self,
        input: &str,
        _ctx: &ParseContext<'_>,
    ) -> Result<Box<dyn Any + Send + Sync>, String> {
        self.parse(input)
    }

    /// Protocol parser hint for client tab-completion.
    /// Return `None` to default to `SingleWord` string.
    fn protocol_parser(&self) -> Option<Parser>;

    /// Number of whitespace-delimited tokens consumed by this parser.
    fn token_count(&self) -> usize {
        1
    }

    /// Server-side tab-completion, used when `suggestions_type` is
    /// `minecraft:ask_server`.
    fn suggestions(&self, _partial: &str, _world: &World) -> Vec<String> {
        vec![]
    }

    /// Minecraft suggestions_type identifier (e.g., "minecraft:ask_server").
    fn suggestions_type(&self) -> Option<&str> {
        None
    }
}

pub const ASK_SERVER: &str = "minecraft:ask_server";

pub(crate) fn starts_with_ignore_case(candidate: &str, partial: &str) -> bool {
    candidate
        .get(..partial.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(partial))
}

fn check_bounds<T: PartialOrd + std::fmt::Display>(
    value: T,
    min: Option<T>,
    max: Option<T>,
) -> Result<T, String> {
    if let Some(min) = min
        && value < min
    {
        return Err(format!("{value} is below minimum {min}"));
    }
    if let Some(max) = max
        && value > max
    {
        return Err(format!("{value} is above maximum {max}"));
    }
    Ok(value)
}

/// Parses as `String` with configurable protocol string type.
pub struct StringArg {
    pub string_type: StringType,
}

impl StringArg {
    pub fn single_word() -> Arc<Self> {
        Arc::new(Self {
            string_type: StringType::SingleWord,
        })
    }

    pub fn quotable() -> Arc<Self> {
        Arc::new(Self {
            string_type: StringType::QuotablePhrase,
        })
    }

    pub fn greedy() -> Arc<Self> {
        Arc::new(Self {
            string_type: StringType::GreedyPhrase,
        })
    }
}

impl ArgParser for StringArg {
    fn type_name(&self) -> &str {
        "string"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        Ok(Box::new(input.to_string()))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::String(self.string_type))
    }
}

/// Greedy string parser — consumes all remaining input as a single `String`.
pub struct GreedyStringArg;

impl ArgParser for GreedyStringArg {
    fn type_name(&self) -> &str {
        "text"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        Ok(Box::new(input.to_string()))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::String(StringType::GreedyPhrase))
    }
}

/// Message argument — parses as `String`, protocol hint = `Message`.
pub struct MessageArg;

impl ArgParser for MessageArg {
    fn type_name(&self) -> &str {
        "message"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        Ok(Box::new(input.to_string()))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::Message)
    }
}

macro_rules! bounded_number_arg {
    ($name:ident, $ty:ty, $parser:ident, $type_name:literal, $label:literal) => {
        pub struct $name {
            pub min: Option<$ty>,
            pub max: Option<$ty>,
        }

        impl $name {
            pub fn new(min: $ty, max: $ty) -> Arc<Self> {
                Arc::new(Self {
                    min: Some(min),
                    max: Some(max),
                })
            }

            pub fn unbounded() -> Arc<Self> {
                Arc::new(Self {
                    min: None,
                    max: None,
                })
            }

            pub fn min(min: $ty) -> Arc<Self> {
                Arc::new(Self {
                    min: Some(min),
                    max: None,
                })
            }
        }

        impl ArgParser for $name {
            fn type_name(&self) -> &str {
                $type_name
            }

            fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
                let value: $ty = input
                    .parse()
                    .map_err(|_| format!("'{}' is not a valid {}", input, $label))?;
                Ok(Box::new(check_bounds(value, self.min, self.max)?))
            }

            fn protocol_parser(&self) -> Option<Parser> {
                Some(Parser::$parser {
                    min: self.min,
                    max: self.max,
                })
            }
        }
    };
}

bounded_number_arg!(IntegerArg, i32, Integer, "integer", "integer");
bounded_number_arg!(LongArg, i64, Long, "long", "long integer");
bounded_number_arg!(FloatArg, f32, Float, "float", "float");
bounded_number_arg!(DoubleArg, f64, Double, "double", "double");

/// Parses `bool` — accepts true/false/yes/no/1/0.
pub struct BoolArg;

impl ArgParser for BoolArg {
    fn type_name(&self) -> &str {
        "boolean"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        match input.to_lowercase().as_str() {
            "true" | "yes" | "1" => Ok(Box::new(true)),
            "false" | "no" | "0" => Ok(Box::new(false)),
            _ => Err(format!(
                "'{}' is not a valid boolean (expected true/false/yes/no/1/0)",
                input
            )),
        }
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::Bool)
    }
}

/// Duration in ticks: `20`, `20t`, `1s` (20 ticks) or `1d` (24000 ticks).
pub struct TimeArg {
    pub min: i32,
}

impl TimeArg {
    pub fn new(min: i32) -> Arc<Self> {
        Arc::new(Self { min })
    }

    pub fn non_negative() -> Arc<Self> {
        Self::new(0)
    }

    pub fn parse_ticks(input: &str) -> Result<i32, String> {
        let unit_start = input
            .find(|c: char| c.is_ascii_alphabetic())
            .unwrap_or(input.len());
        let (number, unit) = input.split_at(unit_start);
        let factor = match unit {
            "" | "t" => 1.0,
            "s" => 20.0,
            "d" => 24000.0,
            _ => {
                return Err(format!(
                    "'{unit}' is not a valid time unit (expected t, s or d)"
                ));
            }
        };
        let value: f32 = number
            .parse()
            .map_err(|_| format!("'{input}' is not a valid time"))?;
        Ok((value * factor).round() as i32)
    }
}

impl ArgParser for TimeArg {
    fn type_name(&self) -> &str {
        "time"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        let ticks = Self::parse_ticks(input)?;
        if ticks < self.min {
            return Err(format!("{ticks} ticks is below minimum {}", self.min));
        }
        Ok(Box::new(ticks))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::Time { min: self.min })
    }
}

/// Parses a `uuid::Uuid` (hyphenated or plain hex).
pub struct UuidArg;

impl ArgParser for UuidArg {
    fn type_name(&self) -> &str {
        "uuid"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        Uuid::parse_str(input)
            .map(|uuid| Box::new(uuid) as Box<dyn Any + Send + Sync>)
            .map_err(|_| format!("'{input}' is not a valid UUID"))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::Uuid)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameMode {
    Survival,
    Creative,
    Adventure,
    Spectator,
}

impl GameMode {
    pub const ALL: [GameMode; 4] = [
        GameMode::Survival,
        GameMode::Creative,
        GameMode::Adventure,
        GameMode::Spectator,
    ];

    pub fn id(self) -> u8 {
        match self {
            GameMode::Survival => 0,
            GameMode::Creative => 1,
            GameMode::Adventure => 2,
            GameMode::Spectator => 3,
        }
    }

    pub fn from_id(id: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.id() == id)
    }

    pub fn name(self) -> &'static str {
        match self {
            GameMode::Survival => "survival",
            GameMode::Creative => "creative",
            GameMode::Adventure => "adventure",
            GameMode::Spectator => "spectator",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|mode| mode.name().eq_ignore_ascii_case(name))
    }
}

/// Parses a `GameMode` by vanilla name (`survival`, ...) or legacy id (`0`-`3`).
pub struct GameModeArg;

impl ArgParser for GameModeArg {
    fn type_name(&self) -> &str {
        "gamemode"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        GameMode::from_name(input)
            .or_else(|| input.parse::<u8>().ok().and_then(GameMode::from_id))
            .map(|mode| Box::new(mode) as Box<dyn Any + Send + Sync>)
            .ok_or_else(|| {
                format!(
                    "'{input}' is not a valid game mode (expected survival, creative, adventure or spectator)"
                )
            })
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::GameMode)
    }
}

/// Parses a `DimensionId` from `minecraft:overworld` / `overworld` and friends.
pub struct DimensionArg;

impl DimensionArg {
    const ALL: [DimensionId; 3] = [
        DimensionId::Overworld,
        DimensionId::Nether,
        DimensionId::End,
    ];
}

impl ArgParser for DimensionArg {
    fn type_name(&self) -> &str {
        "dimension"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        let full = qualify(input);
        Self::ALL
            .into_iter()
            .find(|dimension| dimension.name() == full)
            .map(|dimension| Box::new(dimension) as Box<dyn Any + Send + Sync>)
            .ok_or_else(|| format!("'{input}' is not a known dimension"))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::Dimension)
    }
}

pub const VANILLA_COLOR_NAMES: [&str; 17] = [
    "black",
    "dark_blue",
    "dark_green",
    "dark_aqua",
    "dark_red",
    "dark_purple",
    "gold",
    "gray",
    "dark_gray",
    "blue",
    "green",
    "aqua",
    "red",
    "light_purple",
    "yellow",
    "white",
    "reset",
];

/// Parses one of the 16 vanilla colour names (or `reset`) as a `String`
/// that `MessageRequest::color` accepts directly.
pub struct ColorArg;

impl ArgParser for ColorArg {
    fn type_name(&self) -> &str {
        "color"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        VANILLA_COLOR_NAMES
            .into_iter()
            .find(|name| name.eq_ignore_ascii_case(input))
            .map(|name| Box::new(name.to_string()) as Box<dyn Any + Send + Sync>)
            .ok_or_else(|| format!("'{input}' is not a valid color"))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::Color)
    }
}

/// A fixed set of named variants mapped to values of `T`, completed
/// server-side through `minecraft:ask_server`.
///
/// ```ignore
/// EnumArg::new([("red", Team::Red), ("blue", Team::Blue)])
/// ```
pub struct EnumArg<T> {
    variants: Vec<(String, T)>,
    type_name: String,
}

impl<T: Clone + Send + Sync + 'static> EnumArg<T> {
    pub fn new<N: Into<String>>(variants: impl IntoIterator<Item = (N, T)>) -> Arc<Self> {
        let variants: Vec<(String, T)> = variants
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect();
        assert!(!variants.is_empty(), "EnumArg needs at least one variant");
        let type_name = variants
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join("|");
        Arc::new(Self {
            variants,
            type_name,
        })
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.variants.iter().map(|(name, _)| name.as_str())
    }
}

impl<T: Clone + Send + Sync + 'static> ArgParser for EnumArg<T> {
    fn type_name(&self) -> &str {
        &self.type_name
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        self.variants
            .iter()
            .find(|(name, _)| name == input)
            .or_else(|| {
                self.variants
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(input))
            })
            .map(|(_, value)| Box::new(value.clone()) as Box<dyn Any + Send + Sync>)
            .ok_or_else(|| format!("'{input}' is not one of {}", self.type_name))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::String(StringType::SingleWord))
    }

    fn suggestions(&self, partial: &str, _world: &World) -> Vec<String> {
        self.names()
            .filter(|name| starts_with_ignore_case(name, partial))
            .map(str::to_string)
            .collect()
    }

    fn suggestions_type(&self) -> Option<&str> {
        Some(ASK_SERVER)
    }
}

fn qualify(name: &str) -> String {
    if name.contains(':') {
        name.to_string()
    } else {
        format!("minecraft:{name}")
    }
}

/// Entity selector argument — parses as the raw selector `String`, protocol hint = `Entity`.
pub struct EntityArg {
    pub single: bool,
    pub players_only: bool,
}

impl EntityArg {
    pub fn single_player() -> Arc<Self> {
        Arc::new(Self {
            single: true,
            players_only: true,
        })
    }
}

impl ArgParser for EntityArg {
    fn type_name(&self) -> &str {
        "entity"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        Ok(Box::new(input.to_string()))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::Entity {
            single: self.single,
            players_only: self.players_only,
        })
    }
}

/// Game profile argument — parses as the player name `String`, protocol hint = `GameProfile`,
/// completed server-side with the names of ready players.
pub struct GameProfileArg;

impl ArgParser for GameProfileArg {
    fn type_name(&self) -> &str {
        "player"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        Ok(Box::new(input.to_string()))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::GameProfile)
    }

    fn suggestions(&self, partial: &str, world: &World) -> Vec<String> {
        super::selector::ready_player_names(world)
            .filter(|name| starts_with_ignore_case(name, partial))
            .collect()
    }

    fn suggestions_type(&self) -> Option<&str> {
        Some(ASK_SERVER)
    }
}

/// Resource location argument — parses as `String`, protocol hint = `ResourceLocation`.
/// Validates namespace:path format where both parts must be non-empty and contain only
/// lowercase alphanumeric characters, underscores, hyphens, periods, and forward slashes (path only).
pub struct ResourceLocationArg;

impl ArgParser for ResourceLocationArg {
    fn type_name(&self) -> &str {
        "resource_location"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        let valid_ns_char =
            |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || "_.-".contains(c);
        let valid_path_char =
            |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || "_.-/".contains(c);
        let parts: Vec<&str> = input.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(format!(
                "'{}' is not a valid resource location (expected namespace:path)",
                input
            ));
        }
        let (ns, path) = (parts[0], parts[1]);
        if ns.is_empty() {
            return Err("resource location namespace must not be empty".to_string());
        }
        if path.is_empty() {
            return Err("resource location path must not be empty".to_string());
        }
        if !ns.chars().all(valid_ns_char) {
            return Err(format!("namespace '{}' contains invalid characters", ns));
        }
        if !path.chars().all(valid_path_char) {
            return Err(format!("path '{}' contains invalid characters", path));
        }
        Ok(Box::new(input.to_string()))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::ResourceLocation)
    }
}

/// Summonable entity argument — same namespace:path validation as
/// `ResourceLocationArg` but tells the client to resolve tab-completion
/// suggestions locally from its built-in `minecraft:summonable_entities` list.
pub struct SummonableEntityArg;

impl ArgParser for SummonableEntityArg {
    fn type_name(&self) -> &str {
        "resource_location"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        ResourceLocationArg.parse(input)
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::ResourceLocation)
    }

    fn suggestions_type(&self) -> Option<&str> {
        Some("minecraft:summonable_entities")
    }
}

/// Item argument — `minecraft:stone` or `stone`, resolved to an `ItemId` from
/// the versioned item registry; the client validates and completes it locally.
pub struct ItemArg;

impl ArgParser for ItemArg {
    fn type_name(&self) -> &str {
        "item"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        ItemId::from_name(input)
            .map(|item| Box::new(item) as Box<dyn Any + Send + Sync>)
            .ok_or_else(|| format!("'{input}' is not a known item"))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::ItemStack)
    }
}

/// Block argument — `minecraft:stone` or `stone`, resolved to the block's
/// default block-state id (`i32`) from the versioned block registry.
pub struct BlockArg;

impl ArgParser for BlockArg {
    fn type_name(&self) -> &str {
        "block"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        voidmc_data::block_default_state(voidmc_data::Version::V26_1_2, &qualify(input))
            .map(|state| Box::new(state) as Box<dyn Any + Send + Sync>)
            .ok_or_else(|| format!("'{input}' is not a known block"))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::BlockState)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_as<T: 'static>(parser: &dyn ArgParser, input: &str) -> Result<T, String> {
        parser.parse(input).map(|v| *v.downcast::<T>().unwrap())
    }

    fn parse_rl(s: &str) -> Result<String, String> {
        parse_as::<String>(&ResourceLocationArg, s)
    }

    #[test]
    fn accepts_valid_resource_location() {
        assert_eq!(parse_rl("minecraft:creeper").unwrap(), "minecraft:creeper");
        assert_eq!(parse_rl("mymod:some_entity").unwrap(), "mymod:some_entity");
        assert_eq!(parse_rl("a:b/c").unwrap(), "a:b/c");
    }

    #[test]
    fn rejects_malformed_resource_locations() {
        for input in [
            "creeper",
            ":creeper",
            "minecraft:",
            "a:b:c",
            "Minecraft:Creeper",
        ] {
            assert!(parse_rl(input).is_err(), "{input}");
        }
    }

    #[test]
    fn bounded_numbers_reject_out_of_range() {
        let int = IntegerArg::new(0, 3);
        assert_eq!(parse_as::<i32>(&*int, "2").unwrap(), 2);
        assert!(parse_as::<i32>(&*int, "4").is_err());
        assert!(parse_as::<i32>(&*int, "-1").is_err());
        assert!(parse_as::<i32>(&*int, "x").is_err());

        let long = LongArg::min(10);
        assert_eq!(
            parse_as::<i64>(&*long, "10000000000").unwrap(),
            10_000_000_000
        );
        assert!(parse_as::<i64>(&*long, "9").is_err());

        let float = FloatArg::new(0.0, 1.0);
        assert_eq!(parse_as::<f32>(&*float, "0.5").unwrap(), 0.5);
        assert!(parse_as::<f32>(&*float, "1.5").is_err());

        let double = DoubleArg::unbounded();
        assert_eq!(parse_as::<f64>(&*double, "-2.25").unwrap(), -2.25);
        assert_eq!(
            double.protocol_parser(),
            Some(Parser::Double {
                min: None,
                max: None
            })
        );
    }

    #[test]
    fn bool_accepts_common_spellings() {
        for (input, expected) in [("true", true), ("YES", true), ("0", false), ("no", false)] {
            assert_eq!(parse_as::<bool>(&BoolArg, input).unwrap(), expected);
        }
        assert!(parse_as::<bool>(&BoolArg, "maybe").is_err());
    }

    #[test]
    fn time_parses_units_and_minimum() {
        let time = TimeArg::non_negative();
        assert_eq!(parse_as::<i32>(&*time, "20").unwrap(), 20);
        assert_eq!(parse_as::<i32>(&*time, "20t").unwrap(), 20);
        assert_eq!(parse_as::<i32>(&*time, "1.5s").unwrap(), 30);
        assert_eq!(parse_as::<i32>(&*time, "2d").unwrap(), 48000);
        assert!(parse_as::<i32>(&*time, "-1").is_err());
        assert!(parse_as::<i32>(&*time, "5m").is_err());
        assert!(parse_as::<i32>(&*time, "s").is_err());
        assert_eq!(time.protocol_parser(), Some(Parser::Time { min: 0 }));
    }

    #[test]
    fn uuid_parses_hyphenated_and_plain() {
        let expected = Uuid::parse_str("069a79f4-44e9-4726-a5be-fca90e38aaf5").unwrap();
        assert_eq!(
            parse_as::<Uuid>(&UuidArg, "069a79f4-44e9-4726-a5be-fca90e38aaf5").unwrap(),
            expected
        );
        assert_eq!(
            parse_as::<Uuid>(&UuidArg, "069a79f444e94726a5befca90e38aaf5").unwrap(),
            expected
        );
        assert!(parse_as::<Uuid>(&UuidArg, "not-a-uuid").is_err());
        assert_eq!(UuidArg.protocol_parser(), Some(Parser::Uuid));
    }

    #[test]
    fn gamemode_accepts_names_and_legacy_ids() {
        assert_eq!(
            parse_as::<GameMode>(&GameModeArg, "creative").unwrap(),
            GameMode::Creative
        );
        assert_eq!(
            parse_as::<GameMode>(&GameModeArg, "Spectator").unwrap(),
            GameMode::Spectator
        );
        assert_eq!(
            parse_as::<GameMode>(&GameModeArg, "0").unwrap(),
            GameMode::Survival
        );
        assert!(parse_as::<GameMode>(&GameModeArg, "4").is_err());
        assert!(parse_as::<GameMode>(&GameModeArg, "hardcore").is_err());
        assert_eq!(GameModeArg.protocol_parser(), Some(Parser::GameMode));
    }

    #[test]
    fn dimension_accepts_qualified_and_short_names() {
        assert_eq!(
            parse_as::<DimensionId>(&DimensionArg, "minecraft:the_nether").unwrap(),
            DimensionId::Nether
        );
        assert_eq!(
            parse_as::<DimensionId>(&DimensionArg, "overworld").unwrap(),
            DimensionId::Overworld
        );
        assert!(parse_as::<DimensionId>(&DimensionArg, "minecraft:moon").is_err());
        assert_eq!(DimensionArg.protocol_parser(), Some(Parser::Dimension));
    }

    #[test]
    fn color_accepts_vanilla_names_only() {
        assert_eq!(parse_as::<String>(&ColorArg, "red").unwrap(), "red");
        assert_eq!(
            parse_as::<String>(&ColorArg, "Dark_Aqua").unwrap(),
            "dark_aqua"
        );
        assert_eq!(parse_as::<String>(&ColorArg, "reset").unwrap(), "reset");
        assert!(parse_as::<String>(&ColorArg, "#ff0000").is_err());
        assert!(parse_as::<String>(&ColorArg, "bold").is_err());
        assert_eq!(ColorArg.protocol_parser(), Some(Parser::Color));
    }

    #[test]
    fn item_resolves_to_registry_id() {
        let stone = ItemId::from_name("minecraft:stone").unwrap();
        assert_eq!(
            parse_as::<ItemId>(&ItemArg, "minecraft:stone").unwrap(),
            stone
        );
        assert_eq!(parse_as::<ItemId>(&ItemArg, "stone").unwrap(), stone);
        assert!(parse_as::<ItemId>(&ItemArg, "minecraft:not_an_item").is_err());
        assert_eq!(ItemArg.protocol_parser(), Some(Parser::ItemStack));
    }

    #[test]
    fn block_resolves_to_default_state() {
        let stone = voidmc_data::v26_1_2::blocks::STONE;
        assert_eq!(
            parse_as::<i32>(&BlockArg, "minecraft:stone").unwrap(),
            stone
        );
        assert_eq!(parse_as::<i32>(&BlockArg, "stone").unwrap(), stone);
        assert!(parse_as::<i32>(&BlockArg, "diamond_sword").is_err());
        assert_eq!(BlockArg.protocol_parser(), Some(Parser::BlockState));
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Team {
        Red,
        Blue,
    }

    #[test]
    fn enum_arg_parses_and_completes_variants() {
        let team = EnumArg::new([("red", Team::Red), ("blue", Team::Blue)]);
        assert_eq!(team.type_name(), "red|blue");
        assert_eq!(parse_as::<Team>(&*team, "red").unwrap(), Team::Red);
        assert_eq!(parse_as::<Team>(&*team, "BLUE").unwrap(), Team::Blue);
        assert_eq!(
            parse_as::<Team>(&*team, "green").unwrap_err(),
            "'green' is not one of red|blue"
        );
        assert_eq!(team.suggestions_type(), Some(ASK_SERVER));
        assert_eq!(
            team.protocol_parser(),
            Some(Parser::String(StringType::SingleWord))
        );

        let world = World::new();
        assert_eq!(team.suggestions("b", &world), vec!["blue".to_string()]);
        assert_eq!(team.suggestions("", &world).len(), 2);
    }

    #[test]
    fn game_profile_completes_ready_player_names() {
        use crate::components::{PlayerName, PlayerReady};

        let mut world = World::new();
        world.spawn((PlayerName("Alice".into()), PlayerReady));
        world.spawn((PlayerName("alfred".into()), PlayerReady));
        world.spawn(PlayerName("Alan".into()));

        let mut names = GameProfileArg.suggestions("al", &world);
        names.sort();
        assert_eq!(names, vec!["Alice".to_string(), "alfred".to_string()]);
    }
}
