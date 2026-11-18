use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use super::error::ParseError;
use super::parser::{ArgParser, ParseContext};

/// Definition of a CLI-style flag for a command.
pub struct FlagDefinition {
    pub long: String,
    pub short: Option<char>,
    pub description: String,
    pub takes_value: bool,
    pub value_parser: Option<Arc<dyn ArgParser>>,
}

impl FlagDefinition {
    pub fn matches_token(&self, token: &str) -> bool {
        if let Some(long) = token.strip_prefix("--") {
            return long == self.long;
        }
        let mut short = token.strip_prefix('-').unwrap_or_default().chars();
        match (short.next(), short.next(), self.short) {
            (Some(c), None, Some(expected)) => c == expected,
            _ => false,
        }
    }
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

    fn insert_value(
        &mut self,
        def: &FlagDefinition,
        value: &str,
        ctx: &ParseContext<'_>,
        errors: &mut Vec<ParseError>,
    ) {
        let Some(parser) = &def.value_parser else {
            self.value_flags
                .insert(def.long.clone(), Box::new(value.to_string()));
            return;
        };
        match parser.parse_in(value, ctx) {
            Ok(val) => {
                self.value_flags.insert(def.long.clone(), val);
            }
            Err(detail) => errors.push(ParseError::InvalidValue {
                name: def.long.clone(),
                value: value.to_string(),
                expected: parser.type_name().to_string(),
                detail: Some(detail),
            }),
        }
    }
}

fn is_flag_token(token: &str) -> bool {
    token.starts_with("--")
        || token
            .strip_prefix('-')
            .is_some_and(|short| !short.is_empty() && short.chars().all(|c| c.is_alphabetic()))
}

/// Pre-pass that separates flags from positional tokens.
///
/// - `--flag` sets a boolean flag
/// - `--flag value` sets a value flag (if the flag definition expects a value)
/// - `-f` matches a short flag
/// - `--` stops flag parsing; everything after is positional
/// - A value flag followed by another flag or `--` reports a missing value
///
/// Returns `(positional_tokens, flag_set, errors)`.
pub fn extract_flags(
    tokens: &[String],
    definitions: &[FlagDefinition],
    ctx: &ParseContext<'_>,
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
                    if i + 1 < tokens.len() && !is_flag_token(&tokens[i + 1]) {
                        i += 1;
                        let value_str = &tokens[i];
                        flags.insert_value(def, value_str, ctx, &mut errors);
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
            if is_flag_token(token) {
                let mut matched = true;
                for ch in short_chars.chars() {
                    if let Some(def) = definitions.iter().find(|d| d.short == Some(ch)) {
                        if def.takes_value {
                            // Short value flags must be standalone: -c value
                            if short_chars.len() == 1
                                && i + 1 < tokens.len()
                                && !is_flag_token(&tokens[i + 1])
                            {
                                i += 1;
                                let value_str = &tokens[i];
                                flags.insert_value(def, value_str, ctx, &mut errors);
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

#[cfg(test)]
mod tests {
    use bevy_ecs::world::World;

    use super::*;
    use crate::commands::parser::IntegerArg;

    fn definitions() -> Vec<FlagDefinition> {
        vec![
            FlagDefinition {
                long: "count".into(),
                short: Some('c'),
                description: String::new(),
                takes_value: true,
                value_parser: Some(IntegerArg::unbounded()),
            },
            FlagDefinition {
                long: "verbose".into(),
                short: Some('v'),
                description: String::new(),
                takes_value: false,
                value_parser: None,
            },
            FlagDefinition {
                long: "quiet".into(),
                short: Some('q'),
                description: String::new(),
                takes_value: false,
                value_parser: None,
            },
        ]
    }

    fn extract(input: &[&str]) -> (Vec<String>, FlagSet, Vec<ParseError>) {
        let mut world = World::new();
        let executor = world.spawn_empty().id();
        let ctx = ParseContext {
            world: &world,
            executor,
        };
        let tokens = input
            .iter()
            .map(|token| token.to_string())
            .collect::<Vec<_>>();
        extract_flags(&tokens, &definitions(), &ctx)
    }

    fn assert_missing_count(errors: &[ParseError]) {
        assert!(matches!(
            errors.first(),
            Some(ParseError::FlagMissingValue { flag }) if flag == "count"
        ));
    }

    #[test]
    fn missing_value_does_not_consume_the_next_flag() {
        for count in ["--count", "-c"] {
            for verbose in ["--verbose", "-v", "-vq"] {
                let (positional, flags, errors) = extract(&[count, verbose]);
                assert_missing_count(&errors);
                assert_eq!(errors.len(), 1);
                assert!(positional.is_empty());
                assert!(flags.has("verbose"));
                assert_eq!(flags.has("quiet"), verbose == "-vq");
                assert!(flags.get_value::<i32>("count").is_none());
            }
        }
    }

    #[test]
    fn missing_value_preserves_the_flag_terminator() {
        for count in ["--count", "-c"] {
            let (positional, flags, errors) = extract(&[count, "--", "--verbose"]);
            assert_missing_count(&errors);
            assert_eq!(errors.len(), 1);
            assert_eq!(positional, vec!["--verbose"]);
            assert!(!flags.has("verbose"));
        }
    }

    #[test]
    fn missing_value_preserves_unknown_flag_errors() {
        for count in ["--count", "-c"] {
            for unknown in ["--unknown", "-x"] {
                let (_, _, errors) = extract(&[count, unknown]);
                assert_missing_count(&errors);
                assert_eq!(errors.len(), 2);
                assert!(matches!(errors[1], ParseError::UnknownFlag(_)));
            }
        }
    }

    #[test]
    fn value_flags_still_accept_negative_numbers_and_report_end_of_input() {
        for count in ["--count", "-c"] {
            let (positional, flags, errors) = extract(&[count, "-3", "--verbose", "tail"]);
            assert!(errors.is_empty());
            assert_eq!(flags.get_value::<i32>("count"), Some(&-3));
            assert!(flags.has("verbose"));
            assert_eq!(positional, vec!["tail"]);

            let (_, _, errors) = extract(&[count]);
            assert_missing_count(&errors);
            assert_eq!(errors.len(), 1);
        }
    }
}
