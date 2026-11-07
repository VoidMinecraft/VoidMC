use std::collections::HashSet;
use std::path::Path;

use super::parse::{Line, Scenario};
use crate::registry::Level;
use crate::{check, compare};

pub(super) const ALL: &[Level] = &[Level::Bot, Level::Logic, Level::Render];
const GAME: &[Level] = &[Level::Logic, Level::Render];
const PICTURE: &[Level] = &[Level::Render];

/// Each verb of a scenario, where it runs and how many words it needs at least.
pub(super) const VERBS: &[(&str, &[Level], usize)] = &[
    ("state", ALL, 0),
    ("act", ALL, 1),
    ("wait_until", ALL, 1),
    ("assert", ALL, 1),
    ("entities", ALL, 0),
    ("packets", &[Level::Bot], 0),
    ("turn", GAME, 2),
    ("aim", GAME, 2),
    ("seq", GAME, 2),
    ("attack", GAME, 0),
    ("use", GAME, 0),
    ("shift", GAME, 1),
    ("key", GAME, 2),
    ("hover", GAME, 2),
    ("click", GAME, 2),
    ("free", GAME, 0),
    ("type", GAME, 1),
    ("esc", GAME, 0),
    ("fps", GAME, 1),
    ("dump", GAME, 0),
    ("track", GAME, 1),
    ("frames", GAME, 1),
    ("check", GAME, 1),
    ("clock", GAME, 1),
    ("step", GAME, 1),
    ("fence", GAME, 1),
    ("sent", GAME, 0),
    ("wait_still", GAME, 1),
    ("shot", PICTURE, 1),
    ("compare", PICTURE, 2),
];

impl Scenario {
    /// Everything that can be refused without a client, so a bad scenario never half runs.
    pub fn validate(&self, level: Level, base: &Path) -> Result<(), String> {
        if self.lines.is_empty() {
            return Err("scenario refused: it has no line to play".into());
        }
        let mut errors = Vec::new();
        let mut known = Known::default();
        for line in &self.lines {
            if let Err(error) = line.validate(level, base, &mut known) {
                errors.push(format!("line {}: {error}", line.number));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "scenario refused, nothing was run:\n  {}",
                errors.join("\n  ")
            ))
        }
    }
}

/// What the lines above have promised to make.
#[derive(Default)]
pub(super) struct Known {
    shots: HashSet<String>,
    recordings: HashSet<String>,
    unchecked: HashSet<String>,
}

fn positive(word: &str) -> Result<u64, String> {
    word.parse()
        .ok()
        .filter(|number| *number > 0)
        .ok_or_else(|| format!("{word} is not a positive number"))
}

impl Line {
    fn validate(&self, level: Level, base: &Path, known: &mut Known) -> Result<(), String> {
        let Some((_, levels, least)) = VERBS.iter().find(|(verb, ..)| *verb == self.verb) else {
            return Err(format!("unknown verb {}", self.verb));
        };
        if !levels.contains(&level) {
            let names: Vec<&str> = levels.iter().map(|level| level.name()).collect();
            return Err(format!(
                "{} does not exist at level {}, only at {}",
                self.verb,
                level.name(),
                names.join(", ")
            ));
        }
        if self.words.len() < *least {
            return Err(format!("{} takes at least {least} word(s)", self.verb));
        }
        if let Some((condition, _)) = &self.condition {
            condition.parsed.validate(level)?;
        }
        match self.verb.as_str() {
            "shot" => {
                known.shots.insert(self.words[0].clone());
            }
            "frames" | "track" => {
                positive(&self.words[0])?;
                if !known.unchecked.insert(self.verb.clone()) {
                    return Err(format!(
                        "the {0} above would be lost: check {0} before recording again",
                        self.verb
                    ));
                }
                known.recordings.insert(self.verb.clone());
            }
            "clock" => {
                if !matches!(self.words[0].as_str(), "virtual" | "real") {
                    return Err(format!("clock is virtual or real, not {}", self.words[0]));
                }
                if let Some(recording) = known.unchecked.iter().min()
                    && self.words[0] == "virtual"
                {
                    return Err(format!(
                        "clock virtual would freeze the time under the {recording} still recording, and its check would fail: check {recording} first"
                    ));
                }
            }
            "wait_still" => {
                for word in self.words.iter().take(2) {
                    positive(word)?;
                }
            }
            "compare" => {
                for name in &self.words[..2] {
                    if !known.shots.contains(name) && !base.join(name).exists() {
                        return Err(format!("{name} is neither an earlier shot nor a file"));
                    }
                }
                let rules = compare::Rules::parse(&self.words[2..], base)?;
                if !rules.has_a_limit() {
                    return Err("nothing to hold the pictures to: max=, tile= or require=".into());
                }
            }
            "check" => {
                let word = &self.words[0];
                if matches!(word.as_str(), "frames" | "track") {
                    if !known.recordings.contains(word) {
                        return Err(format!("no {word} line above records anything to check"));
                    }
                    known.unchecked.remove(word);
                } else if !base.join(word).exists() {
                    return Err(format!("{word} is not a file"));
                }
                check::Limits::parse(&self.words[1..], base)?;
            }
            _ => {}
        }
        Ok(())
    }
}
