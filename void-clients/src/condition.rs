use serde_json::Value;

use crate::registry::Level;

pub const DEFAULT_WAIT_MS: u64 = 20_000;
const OPERATORS: [&str; 5] = ["<=", ">=", "==", "!=", "~"];
const ORDERS: [&str; 2] = [">=", "<="];
const TEXT_FIELDS: [&str; 12] = [
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
    "version",
];

fn blank(c: char) -> bool {
    c == ' ' || c == '\t'
}

fn trimmed(text: &str) -> &str {
    text.trim_matches(blank)
}

const FORMAT: [(u32, u32); 21] = [
    (0x00AD, 0x00AD),
    (0x0600, 0x0605),
    (0x061C, 0x061C),
    (0x06DD, 0x06DD),
    (0x070F, 0x070F),
    (0x0890, 0x0891),
    (0x08E2, 0x08E2),
    (0x180E, 0x180E),
    (0x200B, 0x200F),
    (0x202A, 0x202E),
    (0x2060, 0x2064),
    (0x2066, 0x206F),
    (0xFEFF, 0xFEFF),
    (0xFFF9, 0xFFFB),
    (0x110BD, 0x110BD),
    (0x110CD, 0x110CD),
    (0x13430, 0x1343F),
    (0x1BCA0, 0x1BCA3),
    (0x1D173, 0x1D17A),
    (0xE0001, 0xE0001),
    (0xE0020, 0xE007F),
];

fn hidden(c: char) -> bool {
    let format = FORMAT
        .iter()
        .any(|(from, to)| (*from..=*to).contains(&u32::from(c)));
    !blank(c) && (c.is_control() || c.is_whitespace() || format)
}

pub fn split(line: &str) -> (&str, &str) {
    let line = trimmed(line);
    let (name, rest) = line.split_once(blank).unwrap_or((line, ""));
    (name, trimmed(rest))
}

fn value_of(raw: &str, text: &str) -> Result<String, String> {
    if let Some(inner) = raw.strip_prefix('"') {
        let inner = inner
            .strip_suffix('"')
            .filter(|inner| !inner.contains('"'))
            .ok_or_else(|| format!("{text}: a quoted value opens and closes with one \" each"))?;
        if inner.is_empty() {
            return Err(format!("{text} compares with nothing"));
        }
        return Ok(inner.into());
    }
    if raw.is_empty() {
        return Err(format!("{text} compares with nothing"));
    }
    if raw.contains(blank) {
        return Err(format!(
            "{text}: a value is one word, so quote one with spaces, field~\"a b\", and give a wait its limit first, wait_until 5000 condition"
        ));
    }
    if raw.contains('"') {
        return Err(format!("{text}: a quote goes around the whole value"));
    }
    Ok(raw.into())
}

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
    pub fn parse(text: &str) -> Result<Self, String> {
        let bad = || format!("bad condition {text}");
        if text.contains(hidden) {
            return Err(format!(
                "{text:?}: a condition holds visible characters, spaces and tabs, nothing else"
            ));
        }
        let (at, op) = OPERATORS
            .iter()
            .filter_map(|op| text.find(op).map(|at| (at, *op)))
            .min_by_key(|(at, _)| *at)
            .ok_or_else(bad)?;
        let (field, expected) = (trimmed(&text[..at]), trimmed(&text[at + op.len()..]));
        let mut letters = field.chars();
        let starts = letters.next().is_some_and(|c| c.is_ascii_alphabetic());
        if !starts || !letters.all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(bad());
        }
        let expected = value_of(expected, text)?;
        if ORDERS.contains(&op) && !is_number(&expected) {
            return Err(format!("{text}: {op} needs a number, not {expected}"));
        }
        Ok(Self {
            field: field.into(),
            op,
            expected,
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

    pub fn field(&self) -> &str {
        &self.field
    }

    pub fn orders(&self) -> bool {
        ORDERS.contains(&self.op)
    }
}

/// `[ms] condition`: a field starts with a letter, so a first word of digits is the limit and
/// nothing else, and the whole rest is the condition.
pub fn wait(rest: &str) -> Result<(Condition, u64), String> {
    let rest = trimmed(rest);
    if let Some((first, condition)) = rest.split_once(blank)
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

pub fn of(verb: &str, rest: &str) -> Option<Result<(Condition, u64), String>> {
    match verb {
        "assert" => Some(Condition::parse(rest).map(|condition| (condition, DEFAULT_WAIT_MS))),
        "wait_until" => Some(wait(rest)),
        _ => None,
    }
}

pub fn check(verb: &str, level: Level) -> Result<(), String> {
    let (name, rest) = split(verb);
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
            wait("chat~\"a b 12\"").unwrap(),
            (
                Condition::parse("chat~\"a b 12\"").unwrap(),
                DEFAULT_WAIT_MS
            )
        );
        assert!(wait("chat~a b 12").unwrap_err().contains("wait_until 5000"));
        assert!(wait("chunks >= 100 5000").is_err());
        assert!(wait("chunks >=").is_err());
    }

    #[test]
    fn a_quoted_value_keeps_its_spaces_without_its_quotes() {
        let (condition, _) = of("assert", "chat~\"a b 12\"").unwrap().unwrap();
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

    fn case_value(cell: &str, line: &str) -> Value {
        if let Some(text) = cell
            .strip_prefix('"')
            .and_then(|text| text.strip_suffix('"'))
        {
            return json!(text);
        }
        match cell {
            "true" | "false" => json!(cell == "true"),
            _ if is_number(cell) => serde_json::from_str(cell).unwrap(),
            _ => panic!("a value is quoted text, a number, true or false: {line}"),
        }
    }

    fn level_of(name: &str, line: &str) -> Level {
        match name {
            "bot" => Level::Bot,
            "logic" => Level::Logic,
            "render" => Level::Render,
            _ => panic!("no such level in: {line}"),
        }
    }

    fn unescape(cell: &str, line: &str) -> String {
        let mut out = String::new();
        let mut chars = cell.chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    let code = u32::from_str_radix(&hex, 16)
                        .ok()
                        .filter(|_| hex.len() == 4 && hex.chars().all(|c| c.is_ascii_hexdigit()))
                        .and_then(char::from_u32)
                        .unwrap_or_else(|| panic!("an escape is \\t, \\\\ or \\uXXXX: {line}"));
                    out.push(code);
                }
                _ => panic!("an escape is \\t, \\\\ or \\uXXXX: {line}"),
            }
        }
        out
    }

    fn outcome(kind: &str, text: &str, level: Level, fields: &Value) -> Result<String, String> {
        if kind == "split" {
            let (name, rest) = split(text);
            return Ok(format!("{name}|{rest}"));
        }
        let verb = if kind == "assert" {
            "assert"
        } else {
            "wait_until"
        };
        check(&format!("{verb} {text}"), level).and_then(|()| match kind {
            "assert" => Condition::parse(text)?
                .holds(fields)
                .map(|ok| ok.to_string()),
            _ => {
                let (condition, ms) = wait(text)?;
                Ok(format!("{ms} {}", condition.holds(fields)?))
            }
        })
    }

    fn told(result: Result<String, String>, fragment: Option<&str>) -> String {
        match (result, fragment) {
            (_, Some("")) => "a fifth cell with nothing in it".into(),
            (Ok(result), _) => result,
            (Err(message), Some(fragment)) if !message.contains(fragment) => {
                format!("an error without {fragment:?}: {message}")
            }
            (Err(_), _) => "error".into(),
        }
    }

    const CASES: &str = include_str!("../conditions.tsv");

    #[test]
    fn rust_reads_every_shared_case_as_the_table_says() {
        let mut levels = std::collections::HashMap::new();
        let mut wrong = Vec::new();
        let mut cases = 0;
        for line in CASES
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            let owned: Vec<String> = line.split('\t').map(|cell| unescape(cell, line)).collect();
            let cells: Vec<&str> = owned.iter().map(String::as_str).collect();
            if cells[0] == "fields" {
                assert!(
                    cells.len() >= 4 && cells.len().is_multiple_of(2),
                    "a fields line holds a level and pairs: {line}"
                );
                level_of(cells[1], line);
                let fields: serde_json::Map<String, Value> = cells[2..]
                    .chunks(2)
                    .map(|pair| (pair[0].to_string(), case_value(pair[1], line)))
                    .collect();
                levels.insert(cells[1].to_string(), Value::Object(fields));
                continue;
            }
            assert!(
                cells.len() == 4 || (cells.len() == 5 && cells[3] == "error"),
                "a case has four cells, and an error a fifth, not empty, for a piece of its message: {line}"
            );
            assert!(
                matches!(cells[0], "assert" | "wait" | "split"),
                "a case is an assert, a wait or a split: {line}"
            );
            let fields = levels
                .get(cells[1])
                .unwrap_or_else(|| panic!("no fields for the level of: {line}"));
            cases += 1;
            let got = told(
                outcome(cells[0], cells[2], level_of(cells[1], line), fields),
                cells.get(4).copied(),
            );
            if got != cells[3] {
                wrong.push(format!("{line}\tbut Rust says {got}"));
            }
        }
        assert!(cases > 0);
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    #[test]
    fn the_probe_knows_the_same_text_fields() {
        let java = include_str!("../probe/Condition.java");
        let list = java
            .split("TEXT_FIELDS = List.of(")
            .nth(1)
            .and_then(|rest| rest.split(')').next())
            .expect("TEXT_FIELDS in Condition.java");
        let probe: Vec<&str> = list
            .split(',')
            .map(|name| name.trim().trim_matches('"'))
            .collect();
        assert_eq!(probe, TEXT_FIELDS);
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

    fn probe_runner(out: &std::path::Path) -> std::process::Command {
        let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let javac = javac();
        let compiled = std::process::Command::new(&javac)
            .arg("-d")
            .arg(out)
            .arg(crate_dir.join("probe/Condition.java"))
            .arg(crate_dir.join("probe/ConditionCases.java"))
            .output()
            .expect("javac runs");
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let mut run = std::process::Command::new(javac.with_file_name("java"));
        run.arg("-cp").arg(out).arg("ConditionCases");
        run
    }

    #[test]
    fn a_wait_outlasts_a_game_still_loading_within_its_limit() {
        let out = std::env::temp_dir().join(format!("voidc-await-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        let run = probe_runner(&out)
            .arg("--await")
            .output()
            .expect("java runs");
        let _ = std::fs::remove_dir_all(&out);
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "ready after 4 tries, within true\ntimed out on chunks>=1\n"
        );
    }

    #[test]
    fn the_probe_hides_the_same_characters() {
        let out = std::env::temp_dir().join(format!("voidc-hidden-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        let run = probe_runner(&out)
            .arg("--hidden")
            .output()
            .expect("java runs");
        let _ = std::fs::remove_dir_all(&out);
        assert!(run.status.success());
        let probe: Vec<u32> = String::from_utf8_lossy(&run.stdout)
            .lines()
            .map(|line| u32::from_str_radix(line, 16).unwrap())
            .collect();
        let rust: Vec<u32> = (0..=0x10FFFF)
            .filter_map(char::from_u32)
            .filter(|c| hidden(*c))
            .map(u32::from)
            .collect();
        assert!(probe.contains(&0x200B));
        assert_eq!(rust, probe);
    }

    #[test]
    fn a_fifth_cell_left_empty_is_refused_on_both_sides() {
        assert_ne!(told(Err("any message".into()), Some("")), "error");
        let out = std::env::temp_dir().join(format!("voidc-fifth-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        let mut run = probe_runner(&out);
        let table = out.join("table.tsv");
        std::fs::write(
            &table,
            "fields\trender\tchunks\t1\nassert\trender\tchunks>=abc\terror\t\n",
        )
        .unwrap();
        let run = run.arg(&table).output().expect("java runs");
        let _ = std::fs::remove_dir_all(&out);
        assert_eq!(run.status.code(), Some(2));
        let said = String::from_utf8_lossy(&run.stdout);
        assert!(said.contains("not empty"), "{said}");
    }

    #[test]
    fn the_probe_reads_every_shared_case_as_the_table_says() {
        let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let out = std::env::temp_dir().join(format!("voidc-conditions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        let run = probe_runner(&out)
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
