use serde_json::Value;

pub const DEFAULT_WAIT_MS: u64 = 20_000;
const OPERATORS: [&str; 5] = ["<=", ">=", "==", "!=", "~"];

#[derive(Debug, PartialEq)]
pub struct Condition {
    field: String,
    op: &'static str,
    expected: String,
}

impl Condition {
    /// `field op value`, with `>=`, `<=`, `==`, `!=` and `~` for "contains".
    pub fn parse(text: &str) -> Result<Self, String> {
        let bad = || format!("bad condition {text}");
        let (field, op, expected) = OPERATORS
            .iter()
            .find_map(|op| text.split_once(op).map(|(f, e)| (f.trim(), *op, e.trim())))
            .ok_or_else(bad)?;
        if field.is_empty() || !field.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err(bad());
        }
        if expected.is_empty() {
            return Err(format!("{text} compares with nothing"));
        }
        if matches!(op, "<=" | ">=") && expected.parse::<f64>().is_err() {
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
        let wanted = self.expected.parse::<f64>().ok();
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

/// Refuses a verb whose condition is malformed before it reaches a client.
pub fn check(verb: &str) -> Result<(), String> {
    let (name, rest) = verb.trim().split_once(' ').unwrap_or((verb.trim(), ""));
    match name {
        "assert" => Condition::parse(rest).map(drop),
        "wait_until" => wait(rest).map(drop),
        _ => Ok(()),
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
    fn a_missing_field_is_false_and_a_bad_condition_an_error() {
        assert!(!holds("nope>=1", &json!({})).unwrap());
        assert!(holds("chunks", &json!({})).is_err());
    }

    #[test]
    fn an_order_without_numbers_is_an_error_never_true() {
        let fields = json!({"chunks": 1, "state": "connected"});
        assert!(holds("chunks >= 200 30000", &fields).is_err());
        assert!(holds("chunks>=abc", &fields).is_err());
        assert!(holds("chunks >=", &fields).is_err());
        assert!(holds("state>=1", &fields).is_err());
        assert!(holds("state<=1", &fields).is_err());
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
        assert!(wait("chunks >=").is_err());
    }

    #[test]
    fn a_malformed_condition_is_refused_before_it_is_sent() {
        assert!(check("assert chunks >= 200 30000").is_err());
        assert!(check("wait_until chunks >= abc 3000").is_err());
        assert!(check("wait_until chunks >= 100").is_ok());
        assert!(check("shot before").is_ok());
    }
}
