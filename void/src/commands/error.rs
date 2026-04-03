use std::fmt;

/// Errors that can occur during command argument parsing.
pub enum ParseError {
    /// A required argument was not provided.
    MissingArgument { name: String, expected_type: String },
    /// An argument value could not be parsed to the expected type.
    InvalidValue {
        name: String,
        value: String,
        expected: String,
        detail: Option<String>,
    },
    /// A numeric argument was outside the allowed range.
    OutOfRange {
        name: String,
        value: String,
        min: Option<String>,
        max: Option<String>,
    },
    /// An unknown flag was provided.
    UnknownFlag(String),
    /// A flag that requires a value was not given one.
    FlagMissingValue { flag: String },
    /// More positional arguments were provided than expected.
    TooManyArguments { expected: usize, got: usize },
}

impl ParseError {
    /// Format this error as a user-facing message.
    pub fn to_player_message(&self) -> String {
        match self {
            ParseError::MissingArgument {
                name,
                expected_type,
            } => {
                format!(
                    "Missing required argument <{}> (expected {})",
                    name, expected_type
                )
            }
            ParseError::InvalidValue {
                name,
                value,
                expected,
                detail,
            } => {
                let base = format!(
                    "Invalid value '{}' for <{}>: expected {}",
                    value, name, expected
                );
                match detail {
                    Some(d) => format!("{} ({})", base, d),
                    None => base,
                }
            }
            ParseError::OutOfRange {
                name,
                value,
                min,
                max,
            } => {
                let range = match (min, max) {
                    (Some(lo), Some(hi)) => format!("{}..{}", lo, hi),
                    (Some(lo), None) => format!("{}..∞", lo),
                    (None, Some(hi)) => format!("-∞..{}", hi),
                    (None, None) => "any".to_string(),
                };
                format!(
                    "Value '{}' for <{}> is out of range ({})",
                    value, name, range
                )
            }
            ParseError::UnknownFlag(flag) => {
                format!("Unknown flag: --{}", flag)
            }
            ParseError::FlagMissingValue { flag } => {
                format!("Flag --{} requires a value", flag)
            }
            ParseError::TooManyArguments { expected, got } => {
                format!("Too many arguments: expected {}, got {}", expected, got)
            }
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_player_message())
    }
}
