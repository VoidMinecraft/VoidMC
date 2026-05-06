use std::any::Any;
use std::sync::Arc;

use bevy_ecs::world::World;
use voidmc_protocol::clientbound::commands::{Parser, StringType};

/// Trait for argument parsers that both map to the Minecraft protocol
/// command tree (for tab-completion) and actually parse/validate values.
pub trait ArgParser: Send + Sync {
    /// Human-readable type name for error messages (e.g., "integer", "player").
    fn type_name(&self) -> &str;

    /// Parse a string token into a typed value.
    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String>;

    /// Protocol parser hint for client tab-completion.
    /// Return `None` to default to `SingleWord` string.
    fn protocol_parser(&self) -> Option<Parser>;

    /// Tab-completion suggestions (optional, can query ECS world).
    fn suggestions(&self, _partial: &str, _world: &World) -> Vec<String> {
        vec![]
    }

    /// Minecraft suggestions_type identifier (e.g., "minecraft:ask_server").
    fn suggestions_type(&self) -> Option<&str> {
        None
    }
}

// ---------------------------------------------------------------------------
// Built-in parsers
// ---------------------------------------------------------------------------

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

/// Parses `i32` with optional min/max bounds.
pub struct IntegerArg {
    pub min: Option<i32>,
    pub max: Option<i32>,
}

impl IntegerArg {
    pub fn new(min: i32, max: i32) -> Arc<Self> {
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
}

impl ArgParser for IntegerArg {
    fn type_name(&self) -> &str {
        "integer"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        let value: i32 = input
            .parse()
            .map_err(|_| format!("'{}' is not a valid integer", input))?;
        if let Some(min) = self.min {
            if value < min {
                return Err(format!("{} is below minimum {}", value, min));
            }
        }
        if let Some(max) = self.max {
            if value > max {
                return Err(format!("{} is above maximum {}", value, max));
            }
        }
        Ok(Box::new(value))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::Integer {
            min: self.min,
            max: self.max,
        })
    }
}

/// Parses `i64` with optional min/max bounds.
pub struct LongArg {
    pub min: Option<i64>,
    pub max: Option<i64>,
}

impl LongArg {
    pub fn new(min: i64, max: i64) -> Arc<Self> {
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
}

impl ArgParser for LongArg {
    fn type_name(&self) -> &str {
        "long"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        let value: i64 = input
            .parse()
            .map_err(|_| format!("'{}' is not a valid long integer", input))?;
        if let Some(min) = self.min {
            if value < min {
                return Err(format!("{} is below minimum {}", value, min));
            }
        }
        if let Some(max) = self.max {
            if value > max {
                return Err(format!("{} is above maximum {}", value, max));
            }
        }
        Ok(Box::new(value))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::Long {
            min: self.min,
            max: self.max,
        })
    }
}

/// Parses `f32` with optional min/max bounds.
pub struct FloatArg {
    pub min: Option<f32>,
    pub max: Option<f32>,
}

impl FloatArg {
    pub fn new(min: f32, max: f32) -> Arc<Self> {
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
}

impl ArgParser for FloatArg {
    fn type_name(&self) -> &str {
        "float"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        let value: f32 = input
            .parse()
            .map_err(|_| format!("'{}' is not a valid float", input))?;
        if let Some(min) = self.min {
            if value < min {
                return Err(format!("{} is below minimum {}", value, min));
            }
        }
        if let Some(max) = self.max {
            if value > max {
                return Err(format!("{} is above maximum {}", value, max));
            }
        }
        Ok(Box::new(value))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::Float {
            min: self.min,
            max: self.max,
        })
    }
}

/// Parses `f64` with optional min/max bounds.
pub struct DoubleArg {
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl DoubleArg {
    pub fn new(min: f64, max: f64) -> Arc<Self> {
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
}

impl ArgParser for DoubleArg {
    fn type_name(&self) -> &str {
        "double"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        let value: f64 = input
            .parse()
            .map_err(|_| format!("'{}' is not a valid double", input))?;
        if let Some(min) = self.min {
            if value < min {
                return Err(format!("{} is below minimum {}", value, min));
            }
        }
        if let Some(max) = self.max {
            if value > max {
                return Err(format!("{} is above maximum {}", value, max));
            }
        }
        Ok(Box::new(value))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::Double {
            min: self.min,
            max: self.max,
        })
    }
}

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

/// Greedy string parser — consumes all remaining input as a single `String`.
/// Protocol hint is `GreedyPhrase`.
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

/// Entity selector argument — parses as `String`, protocol hint = `Entity`.
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

/// Game profile argument — parses as `String`, protocol hint = `GameProfile`.
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

    fn suggestions_type(&self) -> Option<&str> {
        Some("minecraft:ask_server")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_rl(s: &str) -> Result<String, String> {
        let arg = ResourceLocationArg;
        arg.parse(s).map(|v| *v.downcast::<String>().unwrap())
    }

    #[test]
    fn accepts_valid_resource_location() {
        assert_eq!(parse_rl("minecraft:creeper").unwrap(), "minecraft:creeper");
        assert_eq!(parse_rl("mymod:some_entity").unwrap(), "mymod:some_entity");
        assert_eq!(parse_rl("a:b/c").unwrap(), "a:b/c");
    }

    #[test]
    fn rejects_missing_colon() {
        assert!(parse_rl("creeper").is_err());
    }

    #[test]
    fn rejects_empty_namespace() {
        assert!(parse_rl(":creeper").is_err());
    }

    #[test]
    fn rejects_empty_path() {
        assert!(parse_rl("minecraft:").is_err());
    }

    #[test]
    fn rejects_multiple_colons() {
        assert!(parse_rl("a:b:c").is_err());
    }

    #[test]
    fn rejects_uppercase() {
        assert!(parse_rl("Minecraft:Creeper").is_err());
    }

    #[test]
    fn protocol_parser_is_resource_location() {
        use voidmc_protocol::clientbound::commands::Parser;
        let arg = ResourceLocationArg;
        assert!(matches!(
            arg.protocol_parser(),
            Some(Parser::ResourceLocation)
        ));
    }
}
