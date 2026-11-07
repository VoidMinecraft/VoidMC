use serde_json::Value;

use crate::registry::Level;

pub const DEFAULT_WAIT_MS: u64 = 20_000;
const OPERATORS: [&str; 5] = ["<=", ">=", "==", "!=", "~"];
const ORDERS: [&str; 2] = [">=", "<="];
const TEXT_FIELDS: [&str; 11] = [
    "action_bar",
    "clock",
    "state",
    "screen",
    "level",
    "chat",
    "dimension",
    "last_error",
    "passenger",
    "spectator",
    "shown",
];

/// What the probe takes for a number, `-?\d+(\.\d+)?`: `1e3`, `inf` and `5.` stay words.
pub fn is_number(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, "0"));
    [whole, fraction]
        .iter()
        .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Condition {
    field: String,
    op: &'static str,
    expected: String,
}

impl Condition {
    /// `field op value`, with `>=`, `<=`, `==`, `!=` and `~` for "contains". The first operator
    /// splits it, as in the probe: `chat~a>=b` is a `~`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let bad = || format!("bad condition {text}");
        let (at, op) = OPERATORS
            .iter()
            .filter_map(|op| text.find(op).map(|at| (at, *op)))
            .min_by_key(|(at, _)| *at)
            .ok_or_else(bad)?;
        let (field, expected) = (text[..at].trim(), text[at + op.len()..].trim());
        let mut letters = field.chars();
        let starts = letters.next().is_some_and(|c| c.is_ascii_alphabetic());
        if !starts || !letters.all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(bad());
        }
        if expected.is_empty() {
            return Err(format!("{text} compares with nothing"));
        }
        if ORDERS.contains(&op) && !is_number(expected) {
            return Err(format!("{text}: {op} needs a number, not {expected}"));
        }
        Ok(Self {
            field: field.into(),
            op,
            expected: expected.into(),
        })
    }

    /// False while the field is missing; an order on a value that is not a number is an error.
    pub fn holds(&self, fields: &Value) -> Result<bool, String> {
        let actual = &fields[&self.field];
        if actual.is_null() {
            return Ok(false);
        }
        let text = actual
            .as_str()
            .map_or_else(|| actual.to_string(), str::to_string);
        let number = actual.as_f64();
        let wanted = is_number(&self.expected)
            .then(|| self.expected.parse::<f64>().ok())
            .flatten();
        Ok(match (self.op, number, wanted) {
            ("~", _, _) => text.contains(&self.expected),
            (">=", Some(a), Some(b)) => a >= b,
            ("<=", Some(a), Some(b)) => a <= b,
            (">=" | "<=", _, _) => {
                return Err(format!("{} is {text}, not a number", self.field));
            }
            ("==", Some(a), Some(b)) => a == b,
            ("!=", Some(a), Some(b)) => a != b,
            ("==", _, _) => text == self.expected,
            _ => text != self.expected,
        })
    }

    /// An order on a field that is text at this level is a mistake, refused before any client.
    pub fn validate(&self, level: Level) -> Result<(), String> {
        let text = TEXT_FIELDS.contains(&self.field.as_str())
            || (self.field == "camera" && level != Level::Bot);
        if self.orders() && text {
            return Err(format!(
                "{} is not a number: {} cannot compare it",
                self.field, self.op
            ));
        }
        Ok(())
    }

    fn orders(&self) -> bool {
        ORDERS.contains(&self.op)
    }
}

/// `[ms] condition`: a field starts with a letter, so a first word of digits is the limit and
/// nothing else, and the whole rest is the condition.
pub fn wait(rest: &str) -> Result<(Condition, u64), String> {
    let rest = rest.trim();
    if let Some((first, condition)) = rest.split_once([' ', '\t'])
        && first.bytes().all(|byte| byte.is_ascii_digit())
    {
        if !(1..=9).contains(&first.len()) {
            return Err(format!("bad limit {first}: 1 to 9 digits of milliseconds"));
        }
        let ms = first.parse().map_err(|_| format!("bad limit {first}"))?;
        return Ok((Condition::parse(condition)?, ms));
    }
    Ok((Condition::parse(rest)?, DEFAULT_WAIT_MS))
}

/// The condition of an `assert` or a `wait_until`, with the limit of the wait. An `assert` has
/// no limit, so all that follows its operator is its value. `None` for any other verb.
pub fn of(verb: &str, rest: &str) -> Option<Result<(Condition, u64), String>> {
    match verb {
        "assert" => Some(Condition::parse(rest).map(|condition| (condition, DEFAULT_WAIT_MS))),
        "wait_until" => Some(wait(rest)),
        _ => None,
    }
}

/// Refuses a verb whose condition a scenario would refuse, before it reaches a client.
pub fn check(verb: &str, level: Level) -> Result<(), String> {
    let (name, rest) = verb.trim().split_once(' ').unwrap_or((verb.trim(), ""));
    match of(name, rest) {
        Some(parsed) => parsed?.0.validate(level),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn holds(text: &str, fields: &Value) -> Result<bool, String> {
        Condition::parse(text)?.holds(fields)
    }

    #[test]
    fn conditions_compare_numbers_and_text() {
        let fields = json!({"chunks": 814, "state": "connected", "chat": "Bienvenue sur la carte"});
        assert!(holds("chunks>=800", &fields).unwrap());
        assert!(!holds("chunks<=800", &fields).unwrap());
        assert!(holds("state==connected", &fields).unwrap());
        assert!(holds("state!=disconnected", &fields).unwrap());
        assert!(holds("chat~carte", &fields).unwrap());
        assert!(holds("chunks == 814", &fields).unwrap());
    }

    #[test]
    fn a_condition_reads_as_it_does_in_the_probe() {
        let fields = json!({"chat": "a>=b", "chunks": 1000});
        assert!(holds("chat~a>=b", &fields).unwrap());
        assert!(!holds("chunks==1e3", &fields).unwrap());
        for word in ["1e3", "inf", "5.", ".5", "-", "1.2.3", ""] {
            assert!(!is_number(word), "{word}");
        }
        for number in ["0", "-3", "2.5", "-0.25"] {
            assert!(is_number(number), "{number}");
        }
    }

    #[test]
    fn a_missing_field_is_false_and_a_bad_condition_an_error() {
        assert!(!holds("nope>=1", &json!({})).unwrap());
        assert!(holds("chunks", &json!({})).is_err());
        assert!(holds(">= 3", &json!({})).is_err());
    }

    #[test]
    fn an_order_without_numbers_is_an_error_never_true() {
        let fields = json!({"chunks": 1, "state": "connected", "chat": "hello"});
        assert!(holds("chunks >= 200 30000", &fields).is_err());
        assert!(holds("chunks >= 200 30s", &fields).is_err());
        assert!(holds("chunks>=abc", &fields).is_err());
        assert!(holds("chunks<=1e3", &fields).is_err());
        assert!(holds("chunks >=", &fields).is_err());
        assert!(holds("state>=1", &fields).is_err());
        assert!(holds("chat<=3", &fields).is_err());
        assert!(holds("chunks!=x", &fields).unwrap());
    }

    #[test]
    fn a_spaced_condition_keeps_its_value_and_the_default_limit() {
        assert_eq!(
            wait("chunks >= 100").unwrap(),
            (Condition::parse("chunks>=100").unwrap(), DEFAULT_WAIT_MS)
        );
        assert_eq!(
            wait("5000 chunks >= 100").unwrap(),
            (Condition::parse("chunks>=100").unwrap(), 5000)
        );
        assert_eq!(
            wait("chat~a b 12").unwrap(),
            (Condition::parse("chat~a b 12").unwrap(), DEFAULT_WAIT_MS)
        );
        assert!(wait("chunks >= 100 5000").is_err());
        assert!(wait("chunks >=").is_err());
    }

    #[test]
    fn an_assert_takes_all_after_its_operator_as_its_value() {
        let (condition, _) = of("assert", "chat~a b 12").unwrap().unwrap();
        assert_eq!(condition.expected, "a b 12");
        assert!(of("act", "map").is_none());
    }

    #[test]
    fn a_malformed_condition_is_refused_before_it_is_sent() {
        for level in [Level::Bot, Level::Logic, Level::Render] {
            assert!(check("assert chunks >= 200 30000", level).is_err());
            assert!(check("wait_until chunks >= 200 30s", level).is_err());
            assert!(check("assert chat >= 3", level).is_err());
            assert!(check("assert screen==", level).is_err());
            assert!(check("wait_until chunks >= abc 3000", level).is_err());
            assert!(check("wait_until chunks >= 100", level).is_ok());
            assert!(check("shot before", level).is_ok());
        }
        assert!(check("assert camera>=1", Level::Render).is_err());
        assert!(check("assert camera>=1", Level::Bot).is_ok());
    }

    fn case_value(cell: &str) -> Value {
        match cell
            .strip_prefix('"')
            .and_then(|text| text.strip_suffix('"'))
        {
            Some(text) => json!(text),
            None => serde_json::from_str(cell).unwrap_or_else(|_| panic!("bad value {cell}")),
        }
    }

    fn outcome(kind: &str, text: &str, level: Level, fields: &Value) -> String {
        let verb = if kind == "assert" {
            "assert"
        } else {
            "wait_until"
        };
        let result = check(&format!("{verb} {text}"), level).and_then(|()| match kind {
            "assert" => Condition::parse(text)?
                .holds(fields)
                .map(|ok| ok.to_string()),
            _ => {
                let (condition, ms) = wait(text)?;
                Ok(format!("{ms} {}", condition.holds(fields)?))
            }
        });
        result.unwrap_or_else(|_| "error".into())
    }

    const CASES: &str = include_str!("../conditions.tsv");

    #[test]
    fn rust_reads_every_shared_case_as_the_table_says() {
        let mut levels = std::collections::HashMap::new();
        let mut wrong = Vec::new();
        let mut cases = 0;
        for line in CASES
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        {
            let cells: Vec<&str> = line.split('\t').collect();
            if cells[0] == "fields" {
                let fields: serde_json::Map<String, Value> = cells[2..]
                    .chunks(2)
                    .map(|pair| (pair[0].to_string(), case_value(pair[1])))
                    .collect();
                levels.insert(cells[1], Value::Object(fields));
                continue;
            }
            cases += 1;
            let level = match cells[1] {
                "bot" => Level::Bot,
                "logic" => Level::Logic,
                _ => Level::Render,
            };
            let got = outcome(cells[0], cells[2], level, &levels[cells[1]]);
            if got != cells[3] {
                wrong.push(format!("{line}\tbut Rust says {got}"));
            }
        }
        assert!(cases > 0);
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    fn javac() -> std::path::PathBuf {
        let mut candidates = vec![crate::launch::java_home().join("bin/javac")];
        if let Some(home) = std::env::var_os("JAVA_HOME") {
            candidates.push(std::path::PathBuf::from(home).join("bin/javac"));
        }
        let on_path: Vec<_> = std::env::var_os("PATH")
            .map(|path| {
                std::env::split_paths(&path)
                    .map(|dir| dir.join("javac"))
                    .collect()
            })
            .unwrap_or_default();
        candidates
            .into_iter()
            .chain(on_path)
            .find(|candidate| candidate.is_file())
            .expect("no javac: install a JDK, or the Minecraft runtime that voidc uses, so the probe's conditions can be checked")
    }

    #[test]
    fn the_probe_reads_every_shared_case_as_the_table_says() {
        let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let out = std::env::temp_dir().join(format!("voidc-conditions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        let javac = javac();
        let compiled = std::process::Command::new(&javac)
            .arg("-d")
            .arg(&out)
            .arg(crate_dir.join("probe/Condition.java"))
            .arg(crate_dir.join("probe/ConditionCases.java"))
            .output()
            .expect("javac runs");
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let run = std::process::Command::new(javac.with_file_name("java"))
            .arg("-cp")
            .arg(&out)
            .arg("ConditionCases")
            .arg(crate_dir.join("conditions.tsv"))
            .output()
            .expect("java runs");
        let _ = std::fs::remove_dir_all(&out);
        assert!(
            run.status.success(),
            "{}{}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
    }
}
