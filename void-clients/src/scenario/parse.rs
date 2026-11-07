use crate::condition::{self, Condition};
use crate::registry::Level;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Written {
    pub(super) parsed: Condition,
    pub(super) text: String,
}

impl Written {
    fn of(verb: &str, rest: &str) -> Option<Result<(Self, u64), String>> {
        let parsed = condition::of(verb, rest)?;
        let (first, tail) = condition::split(rest);
        let limited = verb == "wait_until" && first.bytes().all(|byte| byte.is_ascii_digit());
        let text = if limited { tail } else { rest };
        Some(parsed.map(|(parsed, limit)| {
            let text = text.to_string();
            (Self { parsed, text }, limit)
        }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub number: usize,
    pub text: String,
    pub(super) verb: String,
    pub(super) words: Vec<String>,
    pub(super) condition: Option<(Written, u64)>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Scenario {
    pub level: Option<Level>,
    pub lines: Vec<Line>,
}

impl Scenario {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut scenario = Self {
            level: None,
            lines: Vec::new(),
        };
        for (index, raw) in text.lines().enumerate() {
            let number = index + 1;
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let at = |error: String| format!("line {number}: {error}");
            let mut words = line.split_whitespace().map(str::to_string);
            let verb = words.next().unwrap_or_default();
            let words: Vec<String> = words.collect();
            if verb == "level" {
                let name = words.first().ok_or_else(|| at("which level?".into()))?;
                scenario.level = Some(Level::parse(name).map_err(at)?);
                continue;
            }
            let condition = Written::of(&verb, condition::split(line).1)
                .transpose()
                .map_err(at)?;
            scenario.lines.push(Line {
                number,
                text: line.to_string(),
                verb,
                words,
                condition,
            });
        }
        Ok(scenario)
    }

    pub(super) fn conditions(&self) -> impl Iterator<Item = (&Line, &Condition)> {
        self.lines
            .iter()
            .filter_map(|line| Some((line, &line.condition.as_ref()?.0.parsed)))
    }

    pub(super) fn shoots(&self, name: &str) -> bool {
        self.lines
            .iter()
            .any(|line| line.verb == "shot" && line.words.first().is_some_and(|word| word == name))
    }
}
