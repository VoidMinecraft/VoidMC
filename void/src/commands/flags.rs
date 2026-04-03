use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use super::error::ParseError;
use super::parser::ArgParser;

/// Definition of a CLI-style flag for a command.
pub struct FlagDefinition {
    pub long: String,
    pub short: Option<char>,
    pub description: String,
    pub takes_value: bool,
    pub value_parser: Option<Arc<dyn ArgParser>>,
}

/// Parsed flag values extracted from command tokens.
pub struct FlagSet {
    bool_flags: HashMap<String, bool>,
    value_flags: HashMap<String, Box<dyn Any + Send + Sync>>,
}

impl FlagSet {
    fn new() -> Self {
        Self {
            bool_flags: HashMap::new(),
            value_flags: HashMap::new(),
        }
    }

    /// Check if a boolean flag is present.
    pub fn has(&self, name: &str) -> bool {
        self.bool_flags.get(name).copied().unwrap_or(false)
    }

    /// Get a typed flag value.
    pub fn get_value<T: 'static>(&self, name: &str) -> Option<&T> {
        self.value_flags.get(name)?.downcast_ref::<T>()
    }
}

/// Pre-pass that separates flags from positional tokens.
///
/// - `--flag` sets a boolean flag
/// - `--flag value` sets a value flag (if the flag definition expects a value)
/// - `-f` matches a short flag
/// - `--` stops flag parsing; everything after is positional
///
/// Returns `(positional_tokens, flag_set, errors)`.
pub fn extract_flags(
    tokens: &[String],
    definitions: &[FlagDefinition],
) -> (Vec<String>, FlagSet, Vec<ParseError>) {
    let mut positional = Vec::new();
    let mut flags = FlagSet::new();
    let mut errors = Vec::new();
    let mut stop_parsing = false;

    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];

        if stop_parsing {
            positional.push(token.clone());
            i += 1;
            continue;
        }

        if token == "--" {
            stop_parsing = true;
            i += 1;
            continue;
        }

        if let Some(long_name) = token.strip_prefix("--") {
            if let Some(def) = definitions.iter().find(|d| d.long == long_name) {
                if def.takes_value {
                    if i + 1 < tokens.len() {
                        i += 1;
                        let value_str = &tokens[i];
                        if let Some(ref parser) = def.value_parser {
                            match parser.parse(value_str) {
                                Ok(val) => {
                                    flags.value_flags.insert(def.long.clone(), val);
                                }
                                Err(detail) => {
                                    errors.push(ParseError::InvalidValue {
                                        name: def.long.clone(),
                                        value: value_str.clone(),
                                        expected: parser.type_name().to_string(),
                                        detail: Some(detail),
                                    });
                                }
                            }
                        } else {
                            flags
                                .value_flags
                                .insert(def.long.clone(), Box::new(value_str.clone()));
                        }
                    } else {
                        errors.push(ParseError::FlagMissingValue {
                            flag: def.long.clone(),
                        });
                    }
                } else {
                    flags.bool_flags.insert(def.long.clone(), true);
                }
            } else {
                errors.push(ParseError::UnknownFlag(long_name.to_string()));
            }
            i += 1;
            continue;
        }

        if let Some(short_chars) = token.strip_prefix('-') {
            if !short_chars.is_empty() && short_chars.chars().all(|c| c.is_alphabetic()) {
                let mut matched = true;
                for ch in short_chars.chars() {
                    if let Some(def) = definitions.iter().find(|d| d.short == Some(ch)) {
                        if def.takes_value {
                            // Short value flags must be standalone: -c value
                            if short_chars.len() == 1 && i + 1 < tokens.len() {
                                i += 1;
                                let value_str = &tokens[i];
                                if let Some(ref parser) = def.value_parser {
                                    match parser.parse(value_str) {
                                        Ok(val) => {
                                            flags.value_flags.insert(def.long.clone(), val);
                                        }
                                        Err(detail) => {
                                            errors.push(ParseError::InvalidValue {
                                                name: def.long.clone(),
                                                value: value_str.clone(),
                                                expected: parser.type_name().to_string(),
                                                detail: Some(detail),
                                            });
                                        }
                                    }
                                } else {
                                    flags
                                        .value_flags
                                        .insert(def.long.clone(), Box::new(value_str.clone()));
                                }
                            } else if short_chars.len() == 1 {
                                errors.push(ParseError::FlagMissingValue {
                                    flag: def.long.clone(),
                                });
                            } else {
                                // Can't have value flag in a combined short flag group
                                errors.push(ParseError::FlagMissingValue {
                                    flag: def.long.clone(),
                                });
                            }
                        } else {
                            flags.bool_flags.insert(def.long.clone(), true);
                        }
                    } else {
                        matched = false;
                        errors.push(ParseError::UnknownFlag(ch.to_string()));
                    }
                }
                if matched || !errors.is_empty() {
                    i += 1;
                    continue;
                }
            }
        }

        // Regular positional argument
        positional.push(token.clone());
        i += 1;
    }

    (positional, flags, errors)
}
