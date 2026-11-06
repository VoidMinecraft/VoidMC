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
        if field.is_empty() || !field.chars().all(|c| c.is_alphanumeric() || c == '_') {
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

/// `condition [ms]`: a last word is a limit only when what comes before is a whole condition.
pub fn wait(rest: &str) -> Result<(Condition, u64), String> {
    if let Some((before, last)) = rest.trim().rsplit_once(char::is_whitespace)
        && let (Ok(ms), Ok(condition)) = (last.parse::<u64>(), Condition::parse(before))
    {
        return Ok((condition, ms));
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
            wait("chunks >= 100 5000").unwrap(),
            (Condition::parse("chunks>=100").unwrap(), 5000)
        );
        assert_eq!(wait("chat~a b 12").unwrap().1, 12);
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
}
