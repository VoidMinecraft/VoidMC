use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{Value, json};

use crate::condition::{self, Condition, DEFAULT_WAIT_MS};
use crate::registry::{self, Level};
use crate::{check, compare, control, lifecycle};

const ALL: &[Level] = &[Level::Bot, Level::Logic, Level::Render];
const GAME: &[Level] = &[Level::Logic, Level::Render];
const PICTURE: &[Level] = &[Level::Render];
const RECORDING_PATIENCE: Duration = Duration::from_secs(5);
const TRACK_SAMPLE: Duration = Duration::from_millis(5);
const STILL_POLL: Duration = Duration::from_millis(25);
const CAMERA_FIELDS: [&str; 5] = ["cam_x", "cam_y", "cam_z", "cam_yaw", "cam_pitch"];

/// Each verb of a scenario, where it runs and how many words it needs at least.
const VERBS: &[(&str, &[Level], usize)] = &[
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

/// How a scenario sends a condition: as written, and a `wait_until` always carries its limit.
fn verb_of(name: &str, condition: &Written, limit: u64) -> String {
    if name == "assert" {
        format!("assert {}", condition.text)
    } else {
        format!("wait_until {limit} {}", condition.text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Written {
    parsed: Condition,
    text: String,
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
    verb: String,
    words: Vec<String>,
    condition: Option<(Written, u64)>,
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

    fn conditions(&self) -> impl Iterator<Item = (&Line, &Condition)> {
        self.lines
            .iter()
            .filter_map(|line| Some((line, &line.condition.as_ref()?.0.parsed)))
    }

    fn shoots(&self, name: &str) -> bool {
        self.lines
            .iter()
            .any(|line| line.verb == "shot" && line.words.first().is_some_and(|word| word == name))
    }
}

/// What the lines above have promised to make.
#[derive(Default)]
struct Known {
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

#[derive(Debug, Serialize)]
pub struct LineResult {
    line: usize,
    text: String,
    passed: bool,
    ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    got: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<Value>,
}

impl LineResult {
    fn new(line: usize, text: String, ms: u128, step: Step) -> Self {
        Self {
            line,
            text,
            passed: step.error.is_none(),
            ms,
            got: step.got,
            error: step.error,
            note: step.note,
            detail: step.detail,
        }
    }

    fn report(&self) -> String {
        let mut text = format!(
            "{} {:>3} {:>7.3}s {}",
            if self.passed { "ok  " } else { "FAIL" },
            self.line,
            self.ms as f64 / 1000.0,
            self.text
        );
        if let Some(got) = &self.got {
            text.push_str(&format!("  got {got}"));
        }
        if let Some(error) = &self.error {
            text.push_str(&format!("\n     {}", error.replace('\n', "\n     ")));
        }
        if let Some(note) = &self.note {
            text.push_str(&format!("\n     {note}"));
        }
        text
    }
}

#[derive(Debug, Default)]
struct Step {
    got: Option<Value>,
    error: Option<String>,
    note: Option<String>,
    detail: Option<Value>,
}

impl Step {
    fn failed(error: impl Into<String>) -> Self {
        Self {
            error: Some(error.into()),
            ..Self::default()
        }
    }
}

/// A recording that was started, and the line that started it.
struct Recording {
    line: usize,
    text: String,
    lines: Option<usize>,
    patience: Duration,
}

/// What this run has made. Nothing else is read, even if `--out` holds a file of that name.
#[derive(Default)]
struct Made {
    recording: BTreeMap<String, Recording>,
    recorded: HashSet<String>,
    shots: HashSet<String>,
    knows_no_clock: bool,
}

fn restart(client: &str) -> String {
    format!(
        "the probe of {client} is older than this voidc. Restart the client: voidc down {client}, then voidc up"
    )
}

pub struct Run<'a> {
    client: &'a str,
    level: Level,
    base: PathBuf,
    out: PathBuf,
    keep_going: bool,
    patience: Duration,
    made: RefCell<Made>,
}

/// The one thing a scenario talks to, so its runs can be tested without a game.
pub trait Driver {
    fn ask(&mut self, verb: &str) -> Result<Value, String>;
    fn recording(&self, name: &str) -> PathBuf;

    fn read(&self, path: &Path) -> Result<String, String> {
        check::read(path)
    }
}

pub struct Socket(String);

impl Driver for Socket {
    fn ask(&mut self, verb: &str) -> Result<Value, String> {
        let line = control::ask(&self.0, verb, control::timeout_of(verb))?;
        serde_json::from_str(&line).map_err(|e| e.to_string())
    }

    fn recording(&self, name: &str) -> PathBuf {
        registry::client_dir(&self.0).join(format!("{name}.txt"))
    }
}

/// Counts whole lines only: the last one may be half written.
fn line_count(path: &Path) -> usize {
    fs::read(path).map_or(0, |bytes| {
        bytes.iter().filter(|byte| **byte == b'\n').count()
    })
}

/// `frames` and `track` answer at once and write as they go, then leave a `.done` mark.
fn wait_for_recording(verb: &str, path: &Path, recording: &Recording) -> Result<(), String> {
    let done = path.with_extension("done");
    let (mut seen, mut moved) = (0, Instant::now());
    while !done.exists() {
        let now = line_count(path);
        if now != seen {
            seen = now;
            moved = Instant::now();
        }
        if moved.elapsed() >= recording.patience {
            let wanted = recording
                .lines
                .map_or_else(|| "its end".to_string(), |lines| format!("{lines} lines"));
            return Err(format!(
                "{verb} stopped: expected {wanted}, got {seen} lines"
            ));
        }
        sleep(Duration::from_millis(20));
    }
    Ok(())
}

fn whole(verb: &str, text: &str, recording: &Recording) -> Result<(), String> {
    let got = text.matches('\n').count();
    match recording.lines {
        Some(lines) if got < lines => Err(format!(
            "{verb} is truncated: expected {lines} lines, got {got}"
        )),
        _ => Ok(()),
    }
}

fn reply_error(reply: &Value) -> Option<String> {
    (reply["ok"] != true).then(|| {
        reply["error"]
            .as_str()
            .map_or_else(|| reply.to_string(), str::to_string)
    })
}

/// `wait_still <ms> [limit]`: until the camera has not moved for that long. A sleep would only
/// guess when a flight ends, and a picture taken during one compares with nothing.
fn wait_still(driver: &mut impl Driver, line: &Line) -> Step {
    let number = |at: usize, default: u64| {
        line.words
            .get(at)
            .map_or(Some(default), |word| word.parse().ok())
    };
    let (Some(quiet), Some(limit)) = (number(0, 0), number(1, DEFAULT_WAIT_MS)) else {
        return Step::failed("wait_still takes a time and a limit, in ms");
    };
    let start = Instant::now();
    let (mut last, mut since) = (None, Instant::now());
    let mut seen = Value::Null;
    while start.elapsed() < Duration::from_millis(limit) {
        seen = match driver.ask("state") {
            Ok(state) => state,
            Err(error) => return Step::failed(error),
        };
        let view = (seen["ok"] == true)
            .then(|| CAMERA_FIELDS.map(|field| seen[field].as_f64()))
            .filter(|view| view.iter().all(Option::is_some));
        if view.is_none() || view != last {
            last = view;
            since = Instant::now();
        } else if since.elapsed() >= Duration::from_millis(quiet) {
            return Step::default();
        }
        sleep(STILL_POLL);
    }
    let error = match last {
        Some(_) => format!("the camera did not stop within {limit} ms"),
        None => format!("no camera to watch within {limit} ms"),
    };
    Step {
        got: Some(json!(CAMERA_FIELDS.map(|field| seen[field].clone()))),
        error: Some(error),
        ..Step::default()
    }
}

fn condition_step(driver: &mut impl Driver, line: &Line, condition: &Written, limit: u64) -> Step {
    let reply = match driver.ask(&verb_of(&line.verb, condition, limit)) {
        Ok(reply) => reply,
        Err(error) => return Step::failed(error),
    };
    let Some(error) = reply_error(&reply) else {
        return Step::default();
    };
    let got = driver
        .ask("state")
        .ok()
        .and_then(|state| state.get(condition.parsed.field()).cloned());
    let short = error
        .trim_start_matches("java.lang.AssertionError: ")
        .split(" is false:")
        .next()
        .unwrap_or(&error)
        .to_string();
    Step {
        got: Some(got.unwrap_or(Value::Null)),
        error: Some(short),
        ..Step::default()
    }
}

impl<'a> Run<'a> {
    pub fn new(
        client: &'a str,
        level: Level,
        base: PathBuf,
        out: PathBuf,
        keep_going: bool,
    ) -> Self {
        Self {
            client,
            level,
            base,
            out,
            keep_going,
            patience: RECORDING_PATIENCE,
            made: RefCell::default(),
        }
    }

    /// A shot of this run, or a file of the scenario. A name the scenario shoots is only ever
    /// that shot: if it was not taken, there is no picture, whatever lies in the folders.
    fn picture(&self, scenario: &Scenario, name: &str) -> Result<PathBuf, String> {
        if self.made.borrow().shots.contains(name) {
            return Ok(self.out.join("shots").join(format!("{name}.png")));
        }
        if scenario.shoots(name) {
            return Err(format!("shot {name} was not taken in this run"));
        }
        Ok(self.base.join(name))
    }

    fn shot(&self, driver: &mut impl Driver, line: &Line) -> Step {
        let loaded = format!("wait_until {DEFAULT_WAIT_MS} loading==false");
        let reply = driver
            .ask(&loaded)
            .and_then(|reply| match reply_error(&reply) {
                Some(error) => Err(format!("the loading screen stayed: {error}")),
                None => driver.ask(&format!("shot {}", line.words.join(" "))),
            });
        let reply = match reply {
            Ok(reply) => reply,
            Err(error) => return Step::failed(error),
        };
        if let Some(error) = reply_error(&reply) {
            return Step::failed(error);
        }
        let from = PathBuf::from(reply["path"].as_str().unwrap_or_default());
        let dir = self.out.join("shots");
        let to = dir.join(format!("{}.png", line.words[0]));
        match fs::create_dir_all(&dir).and_then(|()| fs::copy(&from, &to)) {
            Ok(_) => {
                self.made.borrow_mut().shots.insert(line.words[0].clone());
                Step {
                    detail: Some(json!({ "path": to })),
                    ..Step::default()
                }
            }
            Err(error) => Step::failed(format!("{}: {error}", from.display())),
        }
    }

    fn compare(&self, scenario: &Scenario, line: &Line) -> Step {
        let name = |word: &str| {
            Path::new(word)
                .file_stem()
                .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned())
        };
        let diff = self.out.join("diffs").join(format!(
            "{}-{}-{}.png",
            line.number,
            name(&line.words[0]),
            name(&line.words[1])
        ));
        let _ = fs::remove_file(&diff);
        let outcome = compare::Rules::parse(&line.words[2..], &self.base).and_then(|rules| {
            compare::files(
                &self.picture(scenario, &line.words[0])?,
                &self.picture(scenario, &line.words[1])?,
                &rules,
                &diff,
            )
        });
        match outcome {
            Err(error) => Step::failed(error),
            Ok(outcome) => Step {
                got: Some(json!(outcome.pixels)),
                error: (!outcome.passed).then(|| outcome.summary()),
                detail: serde_json::to_value(&outcome).ok(),
                ..Step::default()
            },
        }
    }

    fn check(&self, driver: &impl Driver, line: &Line) -> Step {
        let word = &line.words[0];
        if let Err(error) = self.keep_recording(driver, word) {
            return Step::failed(error);
        }
        let path = if matches!(word.as_str(), "frames" | "track") {
            if !self.made.borrow().recorded.contains(word) {
                return Step::failed(format!("{word} was not recorded in this run"));
            }
            self.out.join(format!("{word}.txt"))
        } else {
            self.base.join(word)
        };
        let outcome = check::Limits::parse(&line.words[1..], &self.base)
            .and_then(|limits| check::file(&path, &limits));
        match outcome {
            Err(error) => Step::failed(error),
            Ok(outcome) => Step {
                error: (!outcome.passed).then(|| outcome.summary()),
                note: outcome.note(),
                detail: serde_json::to_value(&outcome).ok(),
                ..Step::default()
            },
        }
    }

    /// Starts a recording and goes on, so that it covers the lines after it.
    fn record(&self, driver: &mut impl Driver, line: &Line) -> Step {
        let path = driver.recording(&line.verb);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(path.with_extension("done"));
        let _ = fs::remove_file(self.out.join(format!("{}.txt", line.verb)));
        self.made.borrow_mut().recorded.remove(&line.verb);
        let step = self.forward(driver, line);
        if step.error.is_none() {
            let samples: u32 = line.words[0].parse().unwrap_or(0);
            let tracked = line.verb == "track";
            self.made.borrow_mut().recording.insert(
                line.verb.clone(),
                Recording {
                    line: line.number,
                    text: line.text.clone(),
                    lines: (!tracked).then_some(samples as usize),
                    patience: self.patience
                        + if tracked {
                            TRACK_SAMPLE * samples
                        } else {
                            Duration::ZERO
                        },
                },
            );
        }
        step
    }

    fn keep_recording(&self, driver: &impl Driver, verb: &str) -> Result<(), String> {
        let Some(recording) = self.made.borrow_mut().recording.remove(verb) else {
            return Ok(());
        };
        let path = driver.recording(verb);
        wait_for_recording(verb, &path, &recording)?;
        let text = driver.read(&path)?;
        whole(verb, &text, &recording)?;
        fs::create_dir_all(&self.out).map_err(|e| e.to_string())?;
        let kept = self.out.join(format!("{verb}.txt"));
        fs::write(&kept, text).map_err(|error| format!("{}: {error}", kept.display()))?;
        self.made.borrow_mut().recorded.insert(verb.to_string());
        Ok(())
    }

    fn clock(&self, driver: &mut impl Driver, line: &Line) -> Step {
        let mut step = self.forward(driver, line);
        if let Some(error) = &step.error
            && error.contains("unknown verb clock")
        {
            self.made.borrow_mut().knows_no_clock = true;
            step.error = Some(format!("{error}: {}", restart(self.client)));
        }
        step
    }

    fn forward(&self, driver: &mut impl Driver, line: &Line) -> Step {
        let verb = format!("{} {}", line.verb, line.words.join(" "));
        match driver.ask(verb.trim()) {
            Ok(reply) => Step {
                error: reply_error(&reply),
                detail: Some(reply),
                ..Step::default()
            },
            Err(error) => Step::failed(error),
        }
    }

    fn step(&self, scenario: &Scenario, driver: &mut impl Driver, line: &Line) -> Step {
        match line.verb.as_str() {
            "shot" => self.shot(driver, line),
            "compare" => self.compare(scenario, line),
            "check" => self.check(driver, line),
            "frames" | "track" => self.record(driver, line),
            "clock" => self.clock(driver, line),
            "wait_still" => wait_still(driver, line),
            "assert" | "wait_until" => {
                let (condition, limit) =
                    line.condition.as_ref().expect("parsed with its condition");
                condition_step(driver, line, condition, *limit)
            }
            _ => self.forward(driver, line),
        }
    }

    /// A field no client has would only ever time out, so it stops the run before its first
    /// verb. Only a client in the world shows all its fields: one that is coming back after a
    /// restart proves nothing, and its run is left to its first `wait_until`.
    fn preflight(&self, scenario: &Scenario, driver: &mut impl Driver) -> Result<(), String> {
        let state = driver.ask("state")?;
        let resets = scenario
            .lines
            .first()
            .is_some_and(|line| line.verb == "clock" && line.words[0] == "real");
        if self.level != Level::Bot && state["ok"] == true && !resets {
            match state["clock"].as_str() {
                Some("virtual") => {
                    return Err(format!(
                        "scenario refused, nothing was run: the clock of {0} is virtual, left so by a run or a command cut short. Every verb could play frozen: send voidc cmd {0} clock real, or start the scenario with clock real",
                        self.client
                    ));
                }
                None => {
                    return Err(format!(
                        "scenario refused, nothing was run: {} does not tell its clock: {}",
                        self.client,
                        restart(self.client)
                    ));
                }
                Some(_) => {}
            }
        }
        if state["ok"] != true || state["state"] != "connected" || state.get("x").is_none() {
            return Ok(());
        }
        let mut errors = Vec::new();
        for (line, condition) in scenario.conditions() {
            let field = condition.field();
            match state.get(field) {
                None => errors.push(format!("line {}: no field {field}", line.number)),
                Some(value) if condition.orders() && !value.is_number() && !value.is_null() => {
                    errors.push(format!("line {}: {field} is not a number", line.number));
                }
                Some(_) => {}
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

    pub fn play(&self, scenario: &Scenario, driver: &mut impl Driver) -> Result<Report, String> {
        self.preflight(scenario, driver)?;
        fs::create_dir_all(&self.out).map_err(|e| format!("{}: {e}", self.out.display()))?;
        let start = Instant::now();
        let mut results = Vec::new();
        for line in &scenario.lines {
            let began = Instant::now();
            let step = self.step(scenario, driver, line);
            let result = LineResult::new(
                line.number,
                line.text.clone(),
                began.elapsed().as_millis(),
                step,
            );
            println!("{}", result.report());
            let stop = !result.passed && !self.keep_going;
            results.push(result);
            if stop {
                break;
            }
        }
        let played = results.len();
        let left: Vec<String> = self.made.borrow().recording.keys().cloned().collect();
        for verb in left {
            let (line, text) = {
                let made = self.made.borrow();
                (
                    made.recording[&verb].line,
                    made.recording[&verb].text.clone(),
                )
            };
            if let Err(error) = self.keep_recording(driver, &verb) {
                let result = LineResult::new(line, text, 0, Step::failed(error));
                println!("{}", result.report());
                results.push(result);
            }
        }
        if self.level != Level::Bot && !self.made.borrow().knows_no_clock {
            results.extend(give_back_clock(self.client, driver));
        }
        let failed = results.iter().filter(|result| !result.passed).count();
        let total = scenario.lines.len();
        Ok(Report {
            client: self.client.to_string(),
            level: self.level.name(),
            total,
            played,
            failed,
            seconds: start.elapsed().as_secs_f64(),
            passed: total > 0 && failed == 0 && played == total,
            lines: results,
        })
    }
}

/// A frozen clock outlives the run on a client that stays up: the next run would play frozen.
fn give_back_clock(client: &str, driver: &mut impl Driver) -> Option<LineResult> {
    let error = driver
        .ask("clock real")
        .map(|reply| reply_error(&reply))
        .unwrap_or_else(Some)?;
    let result = LineResult::new(
        0,
        "clock real, at the end of the run".to_string(),
        0,
        Step::failed(format!(
            "the clock of {client} was not given back, and the next run could play frozen: {error}. Send voidc cmd {client} clock real, or restart the client if its probe does not know clock"
        )),
    );
    println!("{}", result.report());
    Some(result)
}

#[derive(Debug, Serialize)]
pub struct Report {
    client: String,
    level: &'static str,
    total: usize,
    played: usize,
    failed: usize,
    seconds: f64,
    passed: bool,
    lines: Vec<LineResult>,
}

impl Report {
    fn text(&self) -> String {
        let mut text = String::new();
        for line in &self.lines {
            text.push_str(&line.report());
            text.push('\n');
        }
        text.push_str(&self.verdict());
        text.push('\n');
        text
    }

    fn verdict(&self) -> String {
        format!(
            "{}: {} of {} lines played, {} failed, {:.2} s",
            if self.passed { "PASSED" } else { "FAILED" },
            self.played,
            self.total,
            self.failed,
            self.seconds
        )
    }

    fn write(&self, out: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(out.join("report.json"), json).map_err(|e| e.to_string())?;
        fs::write(out.join("report.txt"), self.text()).map_err(|e| e.to_string())
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|at| args.get(at + 1))
        .cloned()
}

fn default_out(client: &str, path: &Path) -> PathBuf {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs());
    registry::client_dir(client)
        .join("runs")
        .join(format!("{stem}-{seconds}"))
}

/// Where the clients come from, so that a whole `voidc run` can be tested without a game.
pub trait Host {
    type Driver: Driver;
    /// The level of a client, and whether it is running.
    fn find(&self, name: &str) -> Option<(Level, bool)>;
    fn up(&mut self, feature: &str, level: Level, port: Option<String>) -> Result<(), String>;
    fn down(&mut self, name: &str) -> Result<(), String>;
    fn driver(&self, name: &str) -> Self::Driver;
}

pub struct Live<F>(pub F);

impl<F> Host for Live<F>
where
    F: FnMut(&str, Level, Option<String>) -> Result<(), String>,
{
    type Driver = Socket;

    fn find(&self, name: &str) -> Option<(Level, bool)> {
        registry::load(name).map(|client| (client.level, registry::alive(&client)))
    }

    fn up(&mut self, feature: &str, level: Level, port: Option<String>) -> Result<(), String> {
        (self.0)(feature, level, port)
    }

    fn down(&mut self, name: &str) -> Result<(), String> {
        lifecycle::down(name)
    }

    fn driver(&self, name: &str) -> Socket {
        Socket(name.to_string())
    }
}

/// Which client plays, and whether this run brings it up.
struct Target {
    name: String,
    level: Level,
    feature: Option<String>,
}

fn target(args: &[String], scenario: &Scenario, host: &impl Host) -> Result<Target, String> {
    let wanted = match flag(args, "--level") {
        Some(level) => Some(Level::parse(&level)?),
        None => scenario.level,
    };
    if let Some(name) = flag(args, "--on") {
        let (level, running) = host
            .find(&name)
            .ok_or_else(|| format!("unknown client {name}"))?;
        if level == Level::Play {
            return Err(format!("{name} is played by Léo and takes no command"));
        }
        if !running {
            return Err(format!("{name} is not running"));
        }
        if let Some(wanted) = wanted.filter(|wanted| *wanted != level) {
            return Err(format!(
                "{name} is a {} client and the scenario wants {}",
                level.name(),
                wanted.name()
            ));
        }
        return Ok(Target {
            name,
            level,
            feature: None,
        });
    }
    let feature =
        flag(args, "--feature").ok_or("which client? --on <name> or --feature \"Feature\"")?;
    let level = wanted.unwrap_or(Level::Render);
    if level == Level::Play {
        return Err("a client to play takes no scenario".into());
    }
    let name = registry::slug(&feature)?;
    if host.find(&name).is_some_and(|(_, running)| running) {
        return Err(format!("{name} is already running: use --on {name}"));
    }
    Ok(Target {
        name,
        level,
        feature: Some(feature),
    })
}

/// `voidc run <file> ...`: a failed line, or a refused scenario, exits with 1.
pub fn command(args: &[String], host: &mut impl Host) -> Result<(), String> {
    let path = PathBuf::from(args.first().ok_or(
        "voidc run <file> (--on <name> | --feature \"Feature\" [--port N]) [--level L] [--keep-going] [--keep-up] [--out DIR]",
    )?);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let scenario = Scenario::parse(&text)?;
    let chosen = target(args, &scenario, host)?;
    let base = path.parent().map_or_else(PathBuf::new, Path::to_path_buf);
    let base = if base.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        base
    };
    scenario.validate(chosen.level, &base)?;
    if let Some(feature) = &chosen.feature
        && let Err(error) = host.up(feature, chosen.level, flag(args, "--port"))
    {
        return match host.down(&chosen.name) {
            Ok(()) => Err(error),
            Err(stuck) => Err(format!("{error}\n{stuck}")),
        };
    }
    let out = flag(args, "--out").map_or_else(|| default_out(&chosen.name, &path), PathBuf::from);
    let run = Run::new(
        &chosen.name,
        chosen.level,
        base,
        out.clone(),
        args.iter().any(|arg| arg == "--keep-going"),
    );
    let played = run.play(&scenario, &mut host.driver(&chosen.name));
    let written = match &played {
        Ok(report) => report.write(&out),
        Err(_) => Ok(()),
    };
    let stopped = if chosen.feature.is_some() && !args.iter().any(|arg| arg == "--keep-up") {
        host.down(&chosen.name)
    } else {
        Ok(())
    };
    let report = played?;
    written?;
    println!("{}", report.verdict());
    println!("report: {}", out.join("report.txt").display());
    stopped?;
    if report.passed {
        Ok(())
    } else {
        Err(String::new())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    struct Fake {
        state: Value,
        replies: VecDeque<Value>,
        asked: Vec<String>,
        dir: PathBuf,
        moving: bool,
        frames: String,
        finishes: bool,
        stale_when_asked: Vec<bool>,
        log: Vec<String>,
        collected_when_released: Vec<bool>,
        untold: bool,
        gone_at_release: bool,
        rewritten_after_read: bool,
    }

    impl Fake {
        fn new(dir: &Path, replies: Vec<Value>) -> Self {
            Self {
                state: json!({
                    "ok": true, "state": "connected", "x": 0.0, "chunks": 71, "camera": "player", "clock": "real",
                    "chat": "", "cam_x": 1.0, "cam_y": 2.0, "cam_z": 3.0, "cam_yaw": 0.0, "cam_pitch": 0.0,
                }),
                replies: replies.into(),
                asked: Vec::new(),
                dir: dir.to_path_buf(),
                moving: false,
                frames: String::new(),
                finishes: true,
                stale_when_asked: Vec::new(),
                log: Vec::new(),
                collected_when_released: Vec::new(),
                untold: false,
                gone_at_release: false,
                rewritten_after_read: false,
            }
        }
    }

    impl Driver for Fake {
        fn ask(&mut self, verb: &str) -> Result<Value, String> {
            self.log.push(verb.to_string());
            if verb == "state" {
                if self.moving {
                    self.state["cam_x"] = json!(self.state["cam_x"].as_f64().unwrap() + 1.0);
                }
                return Ok(self.state.clone());
            }
            self.asked.push(verb.to_string());
            if verb.starts_with("frames") {
                let path = self.recording("frames");
                self.stale_when_asked.push(path.exists());
                fs::write(&path, &self.frames).unwrap();
                if self.finishes {
                    fs::write(path.with_extension("done"), "").unwrap();
                }
            }
            if verb == "clock real" && self.gone_at_release {
                return Err("connection refused".into());
            }
            if verb == "clock real" {
                let collected = self.dir.join("out/frames.txt").exists();
                self.collected_when_released.push(collected);
            }
            let reply = self.replies.pop_front().unwrap_or(json!({"ok": true}));
            if let (Some(mode), true) = (verb.strip_prefix("clock "), reply["ok"] == true)
                && !self.untold
            {
                self.state["clock"] = json!(mode);
            }
            Ok(reply)
        }

        fn recording(&self, name: &str) -> PathBuf {
            self.dir.join(format!("{name}.txt"))
        }

        fn read(&self, path: &Path) -> Result<String, String> {
            let text = check::read(path)?;
            if self.rewritten_after_read {
                fs::write(path, "rewritten\n").unwrap();
            }
            Ok(text)
        }
    }

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("voidc-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn run<'a>(dir: &Path, keep_going: bool) -> Run<'a> {
        let mut run = Run::new(
            "test",
            Level::Render,
            dir.to_path_buf(),
            dir.join("out"),
            keep_going,
        );
        run.patience = Duration::from_millis(100);
        run
    }

    #[test]
    fn a_condition_is_sent_as_written_and_a_wait_carries_its_limit() {
        let scenario = Scenario::parse(
            "wait_until chunks >= 200\nwait_until 30000 chunks >= 200\nassert chat~\"Bienvenue sur  la carte\"\n",
        )
        .unwrap();
        let sent: Vec<String> = scenario
            .lines
            .iter()
            .map(|line| {
                let (condition, limit) = line.condition.as_ref().unwrap();
                verb_of(&line.verb, condition, *limit)
            })
            .collect();
        assert_eq!(
            sent,
            [
                format!("wait_until {DEFAULT_WAIT_MS} chunks >= 200"),
                "wait_until 30000 chunks >= 200".into(),
                "assert chat~\"Bienvenue sur  la carte\"".into(),
            ]
        );
        assert!(Scenario::parse("assert chunks\n").is_err());
        assert!(Scenario::parse("assert >= 3\n").is_err());
        assert!(Scenario::parse("assert chat~Bienvenue sur la carte\n").is_err());
        assert!(Scenario::parse("wait_until chat~Tour 2\n").is_err());
    }

    #[test]
    fn a_quoted_condition_reaches_the_probe_intact_and_reads_the_same() {
        let scenario = Scenario::parse("wait_until 2 action_bar~\"Tour 2\"\n").unwrap();
        let line = &scenario.lines[0];
        let (written, limit) = line.condition.as_ref().unwrap();
        let sent = verb_of(&line.verb, written, *limit);
        assert_eq!(sent, "wait_until 2 action_bar~\"Tour 2\"");
        let (verb, rest) = condition::split(&sent);
        let (reread, reread_limit) = condition::of(verb, rest).unwrap().unwrap();
        assert_eq!((&reread, reread_limit), (&written.parsed, 2));
        let row = format!("wait\trender\t{rest}\t2 false");
        assert!(
            include_str!("../conditions.tsv")
                .lines()
                .any(|case| case == row),
            "the probe reads {rest:?} through conditions.tsv: {row:?}"
        );
    }

    #[test]
    fn comments_and_blank_lines_keep_the_numbers_of_the_file() {
        let scenario = Scenario::parse("# top\n\nlevel bot\nact map\n").unwrap();
        assert_eq!(scenario.level, Some(Level::Bot));
        assert_eq!(scenario.lines[0].number, 4);
    }

    #[test]
    fn a_verb_of_another_level_is_refused_with_every_line_at_fault() {
        let text = "act map\nshot a\ncompare a a max=1\nturn 1 2\nfly\n";
        let scenario = Scenario::parse(text).unwrap();
        let error = scenario.validate(Level::Bot, Path::new(".")).unwrap_err();
        assert!(error.contains("line 2: shot does not exist at level bot, only at render"));
        assert!(error.contains("line 3: compare"));
        assert!(error.contains("line 4: turn"));
        assert!(error.contains("line 5: unknown verb fly"));
        assert!(!error.contains("line 1"));
        assert!(
            scenario
                .validate(Level::Render, Path::new("."))
                .unwrap_err()
                .contains("fly")
        );
    }

    #[test]
    fn the_stepping_verbs_run_on_a_game_and_not_on_a_bot() {
        let scenario = Scenario::parse("clock virtual\nstep 4\nfence 10\nsent\n").unwrap();
        scenario.validate(Level::Render, Path::new(".")).unwrap();
        assert!(scenario.validate(Level::Bot, Path::new(".")).is_err());
    }

    #[test]
    fn a_clock_takes_virtual_or_real_and_nothing_else() {
        let scenario = Scenario::parse("clock lock\n").unwrap();
        let error = scenario
            .validate(Level::Render, Path::new("."))
            .unwrap_err();
        assert!(error.contains("virtual or real"), "{error}");
        let scenario = Scenario::parse("clock real\n").unwrap();
        scenario.validate(Level::Render, Path::new(".")).unwrap();
    }

    #[test]
    fn only_a_virtual_clock_is_refused_while_a_recording_runs() {
        for text in [
            "frames 500\nclock virtual\n",
            "clock virtual\nframes 300\nstep 50 100\nclock real\nclock virtual\n",
            "track 300\nclock virtual\n",
        ] {
            let error = Scenario::parse(text)
                .unwrap()
                .validate(Level::Render, Path::new("."))
                .unwrap_err();
            assert!(error.contains("still recording"), "{text}: {error}");
        }
        for allowed in [
            "clock virtual\nframes 300\nstep 50 100\ncheck frames\nclock real\n",
            "clock virtual\nframes 300\nstep 50 100\nclock real\n",
            "track 300\nclock real\n",
        ] {
            Scenario::parse(allowed)
                .unwrap()
                .validate(Level::Render, Path::new("."))
                .unwrap();
        }
    }

    #[test]
    fn the_stepping_verbs_take_the_words_of_the_probe() {
        for (verb, least) in [("clock", 1), ("step", 1), ("fence", 1), ("sent", 0)] {
            let entry = VERBS.iter().find(|entry| entry.0 == verb).unwrap();
            assert_eq!(entry.2, least, "{verb}");
        }
    }

    #[test]
    fn every_verb_forwarded_to_a_game_exists_in_its_probe() {
        let local = ["check", "compare", "wait_still"];
        for (verb, levels, _) in VERBS {
            let forwarded = levels.contains(&Level::Render) && !local.contains(verb);
            if forwarded {
                assert!(crate::launch::probe_answers(verb), "{verb}");
            }
        }
    }

    #[test]
    fn the_scenario_of_the_repository_is_valid() {
        let text = include_str!("../scenarios/open-map.scn");
        let scenario = Scenario::parse(text).unwrap();
        assert_eq!(scenario.level, Some(Level::Render));
        scenario.validate(Level::Render, Path::new(".")).unwrap();
        assert!(scenario.validate(Level::Logic, Path::new(".")).is_err());
    }

    #[test]
    fn a_scenario_with_nothing_to_play_is_refused_and_never_passes() {
        let scenario = Scenario::parse("# only a comment\nlevel render\n").unwrap();
        assert!(scenario.validate(Level::Render, Path::new(".")).is_err());
        let dir = folder("empty");
        let mut fake = Fake::new(&dir, vec![]);
        assert!(!run(&dir, false).play(&scenario, &mut fake).unwrap().passed);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_number_operator_on_a_text_is_refused_on_every_level() {
        for level in [Level::Bot, Level::Logic, Level::Render] {
            let scenario = Scenario::parse("assert chat >= 5\n").unwrap();
            let error = scenario.validate(level, Path::new(".")).unwrap_err();
            assert!(error.contains("line 1"), "{error}");
        }
        let camera = Scenario::parse("assert camera >= 3\n").unwrap();
        assert!(camera.validate(Level::Render, Path::new(".")).is_err());
        assert!(camera.validate(Level::Bot, Path::new(".")).is_ok());
        let word = Scenario::parse("assert chunks >= many\n").unwrap_err();
        assert!(word.contains("line 1"), "{word}");
        assert!(Scenario::parse("assert chunks >= 1e3\n").is_err());
    }

    #[test]
    fn a_field_that_turns_out_to_be_text_is_refused_before_the_first_verb() {
        let dir = folder("live-text");
        let scenario = Scenario::parse("act map\nassert mood >= 3\n").unwrap();
        let mut fake = Fake::new(&dir, vec![]);
        fake.state["mood"] = json!("calm");
        let error = run(&dir, false).play(&scenario, &mut fake).unwrap_err();
        assert!(error.contains("line 2: mood is not a number"), "{error}");
        assert!(fake.asked.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_client_coming_back_is_not_refused_for_the_fields_it_lacks_for_now() {
        let dir = folder("reconnect");
        let scenario = Scenario::parse("wait_until chunks>=1\n").unwrap();
        for state in [
            json!({"ok": true, "state": "connecting", "screen": "ConnectScreen", "clock": "real"}),
            json!({"ok": true, "state": "connected", "clock": "real"}),
            json!({"ok": false, "error": "game not ready"}),
        ] {
            let mut fake = Fake::new(&dir, vec![]);
            fake.state = state;
            assert!(run(&dir, false).play(&scenario, &mut fake).unwrap().passed);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_camera_that_cannot_be_seen_is_not_a_still_camera() {
        let dir = folder("no-camera");
        let scenario = Scenario::parse("wait_still 10 120\n").unwrap();
        for state in [
            json!({"ok": true, "state": "connecting", "clock": "real"}),
            json!({"ok": false, "error": "game not ready"}),
        ] {
            let mut fake = Fake::new(&dir, vec![]);
            fake.state = state;
            let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
            assert!(!report.passed);
            let error = report.lines[0].error.as_ref().unwrap();
            assert!(error.contains("no camera"), "{error}");
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn bad_numbers_of_a_recording_or_a_wait_are_refused() {
        for text in [
            "frames lots\n",
            "frames 0\n",
            "track x\n",
            "wait_still soon\n",
        ] {
            let scenario = Scenario::parse(text).unwrap();
            assert!(
                scenario.validate(Level::Render, Path::new(".")).is_err(),
                "{text}"
            );
        }
    }

    #[test]
    fn a_check_on_a_file_that_does_not_exist_is_refused() {
        let scenario = Scenario::parse("check nothing.jsonl\n").unwrap();
        assert!(scenario.validate(Level::Render, Path::new(".")).is_err());
    }

    #[test]
    fn a_wait_keeps_its_limit_and_an_assert_has_none() {
        let scenario =
            Scenario::parse("wait_until 12 chat == Score\nassert chat == \"Score 12\"\n").unwrap();
        let (wait, limit) = scenario.lines[0].condition.as_ref().unwrap();
        assert_eq!((wait.text.as_str(), *limit), ("chat == Score", 12));
        let (assertion, _) = scenario.lines[1].condition.as_ref().unwrap();
        assert_eq!(assertion.text, "chat == \"Score 12\"");
        let scenario = Scenario::parse("wait_until chat~\"Tour 2\"\n").unwrap();
        let (wait, limit) = scenario.lines[0].condition.as_ref().unwrap();
        assert_eq!(
            (wait.text.as_str(), *limit),
            ("chat~\"Tour 2\"", DEFAULT_WAIT_MS)
        );
    }

    #[test]
    fn a_comparison_must_name_real_pictures_and_a_limit() {
        let scenario = Scenario::parse("compare before after max=1\n").unwrap();
        let error = scenario
            .validate(Level::Render, Path::new("."))
            .unwrap_err();
        assert!(error.contains("before is neither"));
        let scenario = Scenario::parse("shot a\ncompare a a\n").unwrap();
        let error = scenario
            .validate(Level::Render, Path::new("."))
            .unwrap_err();
        assert!(error.contains("nothing to hold"));
    }

    #[test]
    fn a_shot_after_its_comparison_is_too_late() {
        let scenario = Scenario::parse("compare a a max=1\nshot a\n").unwrap();
        assert!(scenario.validate(Level::Render, Path::new(".")).is_err());
    }

    #[test]
    fn a_failed_assert_gives_its_line_and_what_was_received() {
        let dir = folder("assert");
        let scenario = Scenario::parse("# open\nassert chunks>=900\nact map\n").unwrap();
        let refusal = json!({"ok": false, "error": "chunks>=900 is false: {...}"});
        let mut fake = Fake::new(&dir, vec![refusal]);
        let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
        assert!(!report.passed);
        assert_eq!((report.played, report.total), (1, 2));
        assert_eq!(report.lines[0].line, 2);
        assert_eq!(report.lines[0].got, Some(json!(71)));
        assert_eq!(fake.asked, ["assert chunks>=900", "clock real"]);
        assert!(report.text().contains("FAIL"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_clock_left_virtual_goes_back_to_real_even_after_a_failure() {
        let dir = folder("clock-left");
        let scenario = Scenario::parse("clock virtual\nassert chunks>=900\nact map\n").unwrap();
        let refusal = json!({"ok": false, "error": "chunks>=900 is false: {...}"});
        let mut fake = Fake::new(&dir, vec![json!({"ok": true}), refusal]);
        let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
        assert!(!report.passed);
        assert_eq!(report.played, 2);
        assert_eq!(fake.asked.last().unwrap(), "clock real");
        assert_eq!(fake.state["clock"], "real");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn every_run_on_a_game_gives_the_clock_back_whatever_it_played() {
        for (name, text) in [
            ("clock-back", "clock virtual\nclock real\n"),
            ("clock-never", "act map\n"),
            ("clock-real-only", "clock real\n"),
        ] {
            let dir = folder(name);
            let scenario = Scenario::parse(text).unwrap();
            let mut fake = Fake::new(&dir, vec![]);
            let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
            assert!(report.passed, "{}", report.text());
            assert_eq!(report.lines.len(), scenario.lines.len());
            assert_eq!(fake.asked.last().unwrap(), "clock real");
            let released = fake.asked.iter().filter(|verb| *verb == "clock real");
            assert_eq!(released.count(), text.matches("clock real").count() + 1);
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn a_clock_that_cannot_go_back_fails_the_run() {
        let dir = folder("clock-stuck");
        let scenario = Scenario::parse("clock virtual\n").unwrap();
        let refused = json!({"ok": false, "error": "no"});
        let mut fake = Fake::new(&dir, vec![json!({"ok": true}), refused]);
        let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
        assert!(!report.passed);
        assert_eq!(report.failed, 1);
        let error = report.lines[1].error.as_ref().unwrap();
        assert!(
            error.contains("was not given back") && error.contains("voidc cmd test clock real"),
            "{error}"
        );
        assert!(report.text().contains("clock real, at the end of the run"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_client_gone_at_the_end_fails_the_run() {
        let dir = folder("clock-gone");
        let scenario = Scenario::parse("act map\n").unwrap();
        let mut fake = Fake::new(&dir, vec![]);
        fake.gone_at_release = true;
        let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
        assert!(!report.passed);
        assert_eq!(report.failed, 1);
        let error = report.lines.last().unwrap().error.as_ref().unwrap();
        assert!(
            error.contains("was not given back") && error.contains("connection refused"),
            "{error}"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_probe_that_knows_no_clock_fails_the_run_and_is_told_to_restart() {
        let dir = folder("clock-unknown");
        let scenario = Scenario::parse("act map\n").unwrap();
        let unknown = json!({"ok": false, "error": "unknown verb clock"});
        let mut fake = Fake::new(&dir, vec![json!({"ok": true}), unknown]);
        fake.state = json!({"ok": false, "error": "game not ready"});
        let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
        assert!(!report.passed);
        assert_eq!(fake.asked, ["act map", "clock real"]);
        let error = report.lines.last().unwrap().error.as_ref().unwrap();
        assert!(
            error.contains("unknown verb clock") && error.contains("restart the client"),
            "{error}"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_probe_that_knows_no_clock_is_told_to_restart_at_its_first_clock_line_only() {
        let dir = folder("clock-unknown-reset");
        let scenario = Scenario::parse("clock real\nact map\n").unwrap();
        let unknown =
            json!({"ok": false, "error": "java.lang.IllegalArgumentException: unknown verb clock"});
        let mut fake = Fake::new(&dir, vec![unknown]);
        fake.state.as_object_mut().unwrap().remove("clock");
        let report = run(&dir, true).play(&scenario, &mut fake).unwrap();
        assert!(!report.passed);
        assert_eq!(report.failed, 1, "{}", report.text());
        let error = report.lines[0].error.as_ref().unwrap();
        assert!(
            error.contains("unknown verb clock") && error.contains("Restart the client"),
            "{error}"
        );
        assert_eq!(fake.asked, ["clock real", "act map"]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failed_clock_real_is_given_back_at_the_end_all_the_same() {
        let dir = folder("clock-real-failed");
        let scenario = Scenario::parse("clock virtual\nclock real\n").unwrap();
        let refused = json!({"ok": false, "error": "no"});
        let mut fake = Fake::new(&dir, vec![json!({"ok": true}), refused]);
        let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
        assert!(!report.passed);
        assert_eq!(fake.asked, ["clock virtual", "clock real", "clock real"]);
        assert_eq!(fake.state["clock"], "real");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_run_stopped_before_its_clock_line_still_gives_the_clock_back() {
        let dir = folder("clock-unplayed");
        let scenario = Scenario::parse("assert chunks>=900\nclock virtual\n").unwrap();
        let refusal = json!({"ok": false, "error": "chunks>=900 is false: {...}"});
        let mut fake = Fake::new(&dir, vec![refusal]);
        let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
        assert_eq!((report.played, report.lines.len()), (1, 1));
        assert_eq!(
            fake.log,
            ["state", "assert chunks>=900", "state", "clock real"]
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_clock_is_given_back_only_once_the_recordings_are_in() {
        let dir = folder("clock-after-recording");
        let scenario = Scenario::parse("clock virtual\nframes 2\n").unwrap();
        let mut fake = Fake::new(&dir, vec![]);
        fake.frames = "1000000 1 2 70 12 true 0 0\n1010000 1 2 70 12 true 0 0\n".into();
        let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
        assert!(report.passed, "{}", report.text());
        assert_eq!(fake.collected_when_released, [true]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_recording_the_probe_marked_invalid_fails_the_run_and_is_not_kept() {
        let source = include_str!("../probe/Probe.java");
        let (_, rest) = source.split_once("SPOILED = \"").unwrap();
        let (spoiled, _) = rest.split_once('"').unwrap();
        for (name, text) in [
            ("invalid-unchecked", "clock virtual\nframes 2\nclock real\n"),
            (
                "invalid-checked",
                "clock virtual\nframes 2\nclock real\ncheck frames\n",
            ),
        ] {
            let dir = folder(name);
            fs::create_dir_all(dir.join("out")).unwrap();
            fs::write(dir.join("out/frames.txt"), "1 2 3\n").unwrap();
            let scenario = Scenario::parse(text).unwrap();
            scenario.validate(Level::Render, &dir).unwrap();
            let mut fake = Fake::new(&dir, vec![]);
            fake.frames =
                format!("1000000 1 2 70 12 true 0 0\n1010000 1 2 70 12 true 0 0\n{spoiled}\n");
            let report = run(&dir, true).play(&scenario, &mut fake).unwrap();
            assert!(!report.passed, "{name}: {}", report.text());
            let error = report
                .lines
                .iter()
                .find_map(|line| line.error.as_ref())
                .unwrap();
            assert!(
                error.contains("is invalid") && error.contains("went back to real time"),
                "{name}: {error}"
            );
            assert!(!dir.join("out/frames.txt").exists(), "{name}");
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn what_is_kept_is_the_text_that_was_read() {
        let dir = folder("kept-as-read");
        let scenario = Scenario::parse("frames 2\n").unwrap();
        let mut fake = Fake::new(&dir, vec![]);
        fake.frames = "1000000 1 2 70 12 true 0 0\n1010000 1 2 70 12 true 0 0\n".into();
        fake.rewritten_after_read = true;
        let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
        assert!(report.passed, "{}", report.text());
        assert_eq!(
            fs::read_to_string(dir.join("out/frames.txt")).unwrap(),
            fake.frames
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_bot_is_neither_asked_its_clock_nor_given_it_back() {
        let dir = folder("clock-bot");
        let scenario = Scenario::parse("act map\n").unwrap();
        let mut fake = Fake::new(&dir, vec![]);
        fake.state.as_object_mut().unwrap().remove("clock");
        let mut bot = run(&dir, false);
        bot.level = Level::Bot;
        let report = bot.play(&scenario, &mut fake).unwrap();
        assert!(report.passed, "{}", report.text());
        assert_eq!(fake.asked, ["act map"]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_clock_found_virtual_stops_the_run_before_its_first_verb() {
        let dir = folder("clock-found");
        let scenario = Scenario::parse("clock virtual\nact map\n").unwrap();
        let mut fake = Fake::new(&dir, vec![]);
        fake.state["clock"] = json!("virtual");
        fake.state["state"] = json!("connecting");
        let error = run(&dir, false).play(&scenario, &mut fake).unwrap_err();
        assert!(error.contains("voidc cmd test clock real"), "{error}");
        assert!(fake.asked.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_client_that_does_not_tell_its_clock_gets_it_back_without_a_guess() {
        let dir = folder("clock-untold");
        let text = "clock real\nclock virtual\nact map\nclock real\n";
        let scenario = Scenario::parse(text).unwrap();
        let mut fake = Fake::new(&dir, vec![]);
        fake.state.as_object_mut().unwrap().remove("clock");
        fake.untold = true;
        let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
        assert!(report.passed, "{}", report.text());
        assert_eq!(
            fake.asked,
            [
                "clock real",
                "clock virtual",
                "act map",
                "clock real",
                "clock real"
            ]
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_client_that_does_not_tell_its_clock_is_refused_unless_the_scenario_resets_it() {
        let dir = folder("clock-untold-preflight");
        let mut fake = Fake::new(&dir, vec![]);
        fake.state.as_object_mut().unwrap().remove("clock");
        fake.untold = true;
        let scenario = Scenario::parse("act map\n").unwrap();
        let error = run(&dir, false).play(&scenario, &mut fake).unwrap_err();
        assert!(
            error.contains("does not tell its clock") && error.contains("Restart the client"),
            "{error}"
        );
        assert!(!error.contains("cmd test clock real"), "{error}");
        assert!(fake.asked.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_scenario_that_starts_with_clock_real_may_take_a_frozen_client() {
        let dir = folder("clock-reset");
        let scenario = Scenario::parse("clock real\nact map\n").unwrap();
        let mut fake = Fake::new(&dir, vec![]);
        fake.state["clock"] = json!("virtual");
        let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
        assert!(report.passed, "{}", report.text());
        assert_eq!(fake.asked, ["clock real", "act map", "clock real"]);
        assert_eq!(fake.state["clock"], "real");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn keep_going_plays_every_line_and_still_fails() {
        let dir = folder("keep");
        let scenario = Scenario::parse("assert chunks>=900\nact map\n").unwrap();
        let refusal = json!({"ok": false, "error": "no"});
        let mut fake = Fake::new(&dir, vec![refusal]);
        let report = run(&dir, true).play(&scenario, &mut fake).unwrap();
        assert_eq!(report.played, 2);
        assert_eq!(report.failed, 1);
        assert!(!report.passed);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_wait_always_carries_its_limit_so_a_value_is_never_taken_for_one() {
        let dir = folder("wait");
        let scenario =
            Scenario::parse("wait_until chunks >= 200\nwait_until 500 chunks>=1\n").unwrap();
        let mut fake = Fake::new(&dir, vec![]);
        run(&dir, false).play(&scenario, &mut fake).unwrap();
        assert_eq!(
            fake.asked,
            [
                "wait_until 20000 chunks >= 200",
                "wait_until 500 chunks>=1",
                "clock real"
            ]
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_field_no_client_has_stops_the_run_before_its_first_verb() {
        let dir = folder("field");
        let scenario = Scenario::parse("act map\nwait_until chunkz>=1\n").unwrap();
        let mut fake = Fake::new(&dir, vec![]);
        let error = run(&dir, false).play(&scenario, &mut fake).unwrap_err();
        assert!(error.contains("line 2: no field chunkz"));
        assert!(fake.asked.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_shot_is_kept_and_compared_by_its_name() {
        let dir = folder("shot");
        let taken = dir.join("taken.png");
        let picture = compare::Picture {
            width: 4,
            height: 4,
            rgb: vec![9; 48],
        };
        picture.save(&taken).unwrap();
        picture.save(&dir.join("ref.png")).unwrap();
        let scenario = Scenario::parse("shot a\ncompare a ref.png max=0\n").unwrap();
        let reply = json!({"ok": true, "path": taken});
        let mut fake = Fake::new(&dir, vec![json!({"ok": true}), reply]);
        let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
        assert!(report.passed, "{}", report.text());
        assert_eq!(fake.asked[0], "wait_until 20000 loading==false");
        assert!(dir.join("out/shots/a.png").exists());
        assert!(dir.join("out/diffs/2-a-ref.png").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_recording_runs_during_the_next_lines_and_is_kept_when_checked() {
        let dir = folder("frames");
        fs::write(dir.join("frames.txt"), "stale").unwrap();
        let scenario = Scenario::parse("frames 2\nact map\ncheck frames\n").unwrap();
        let mut fake = Fake::new(&dir, vec![]);
        fake.frames = "1000000 1 2 70 12 true 0 0\n1010000 90 2 70 12 true 0 0\n".into();
        let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
        assert_eq!(fake.asked, ["frames 2", "act map", "clock real"]);
        assert_eq!(fake.stale_when_asked, [false]);
        assert!(!report.passed);
        let error = report.lines[2].error.as_ref().unwrap();
        assert!(error.contains("jump past 64 blocks"), "{error}");
        assert!(
            fs::read_to_string(dir.join("out/frames.txt"))
                .unwrap()
                .starts_with("1000000")
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_recording_that_never_ends_fails_with_what_it_got() {
        let dir = folder("unfinished");
        let scenario = Scenario::parse("frames 500\ncheck frames\n").unwrap();
        let mut fake = Fake::new(&dir, vec![]);
        fake.frames = "1000000 1 2 70 12 true 0 0\n".into();
        fake.finishes = false;
        let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
        let error = report.lines[1].error.as_ref().unwrap();
        assert!(
            error.contains("500") && error.contains("1 lines"),
            "{error}"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_truncated_recording_fails_the_run_even_if_nothing_checks_it() {
        let dir = folder("unchecked");
        let scenario = Scenario::parse("frames 500\n").unwrap();
        let mut fake = Fake::new(&dir, vec![]);
        fake.frames = "1000000 1 2 70 12 true 0 0\n".into();
        let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
        assert!(!report.passed);
        assert_eq!((report.played, report.failed), (1, 1));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn track_waits_for_the_end_of_the_writing_and_not_for_a_first_line() {
        let dir = folder("track");
        let path = dir.join("track.txt");
        fs::write(&path, "1000 1 2\n").unwrap();
        let recording = Recording {
            line: 1,
            text: "track 10".into(),
            lines: None,
            patience: Duration::from_millis(60),
        };
        let error = wait_for_recording("track", &path, &recording).unwrap_err();
        assert!(error.contains("its end"), "{error}");
        fs::write(dir.join("track.done"), "").unwrap();
        assert!(wait_for_recording("track", &path, &recording).is_ok());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_half_written_last_line_does_not_count() {
        let dir = folder("half");
        let path = dir.join("frames.txt");
        fs::write(&path, "1 2 3\n4 5").unwrap();
        assert_eq!(line_count(&path), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn what_a_green_check_did_not_look_at_is_said() {
        let dir = folder("green");
        let scenario = Scenario::parse("frames 2\ncheck frames\n").unwrap();
        let mut fake = Fake::new(&dir, vec![]);
        fake.frames = "1000000 1 2 70 12 true 0 0\n1010000 1 2 70 12 true 0 0\n".into();
        let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
        assert!(report.passed, "{}", report.text());
        assert!(
            report.text().contains("not checked: flashes"),
            "{}",
            report.text()
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_recording_nobody_checked_is_still_kept() {
        let dir = folder("kept");
        let scenario = Scenario::parse("frames 1\n").unwrap();
        let mut fake = Fake::new(&dir, vec![]);
        fake.frames = "1000000 1 2 70 12 true 0 0\n".into();
        run(&dir, false).play(&scenario, &mut fake).unwrap();
        assert!(dir.join("out/frames.txt").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn checking_a_recording_nobody_made_is_refused() {
        let scenario = Scenario::parse("act map\ncheck frames\n").unwrap();
        let error = scenario
            .validate(Level::Render, Path::new("."))
            .expect_err("nothing records frames here");
        assert!(error.contains("line 2"), "{error}");
    }

    #[test]
    fn an_old_recording_of_the_client_is_never_read() {
        let dir = folder("old-recording");
        fs::write(dir.join("frames.txt"), "1000000 1 2 70 12 true 0 0\n").unwrap();
        let scenario = Scenario::parse("check frames\n").unwrap();
        let mut fake = Fake::new(&dir, vec![]);
        let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
        assert!(!report.passed);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_truncated_recording_fails_with_both_counts() {
        let dir = folder("truncated");
        let scenario = Scenario::parse("frames 500\ncheck frames\n").unwrap();
        let mut fake = Fake::new(&dir, vec![]);
        fake.frames = "1000000 1 2 70 12 true 0 0\n1010000 1 2 70 12 true 0 0\n".into();
        let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
        assert!(!report.passed);
        let error = report.lines[1].error.as_ref().unwrap();
        assert!(error.contains("500") && error.contains('2'), "{error}");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_assert_keeps_its_last_word() {
        let dir = folder("assert-word");
        let scenario = Scenario::parse("assert chat == \"Score 12\"\n").unwrap();
        let mut fake = Fake::new(&dir, vec![]);
        fake.state["chat"] = json!("Score 12");
        run(&dir, false).play(&scenario, &mut fake).unwrap();
        assert_eq!(fake.asked, ["assert chat == \"Score 12\"", "clock real"]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_reused_out_folder_gives_no_old_shot() {
        let dir = folder("stale-shot");
        fs::create_dir_all(dir.join("out/shots")).unwrap();
        let picture = compare::Picture {
            width: 4,
            height: 4,
            rgb: vec![9; 48],
        };
        picture.save(&dir.join("out/shots/a.png")).unwrap();
        picture.save(&dir.join("ref.png")).unwrap();
        let scenario = Scenario::parse("shot a\ncompare a ref.png max=0\n").unwrap();
        let refused = json!({"ok": false, "error": "no picture"});
        let mut fake = Fake::new(&dir, vec![json!({"ok": true}), refused]);
        let report = run(&dir, true).play(&scenario, &mut fake).unwrap();
        assert_eq!(report.failed, 2, "{}", report.text());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_reused_out_folder_gives_no_old_recording() {
        let dir = folder("stale-frames");
        fs::create_dir_all(dir.join("out")).unwrap();
        fs::write(dir.join("out/frames.txt"), "1000000 1 2 70 12 true 0 0\n").unwrap();
        let scenario = Scenario::parse("frames 1\ncheck frames\n").unwrap();
        let refused = json!({"ok": false, "error": "not now"});
        let mut fake = Fake::new(&dir, vec![refused]);
        let report = run(&dir, true).play(&scenario, &mut fake).unwrap();
        assert_eq!(report.failed, 2, "{}", report.text());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_second_recording_waits_for_the_first_to_be_checked() {
        for verb in ["frames", "track"] {
            let twice = format!("{verb} 10\nact map\n{verb} 10\ncheck {verb}\n");
            let scenario = Scenario::parse(&twice).unwrap();
            let error = scenario
                .validate(Level::Render, Path::new("."))
                .unwrap_err();
            assert!(error.contains("line 3"), "{error}");
            let checked = format!("{verb} 10\ncheck {verb}\n{verb} 10\ncheck {verb}\n");
            let scenario = Scenario::parse(&checked).unwrap();
            scenario.validate(Level::Render, Path::new(".")).unwrap();
        }
        let both = Scenario::parse("frames 10\ntrack 10\ncheck frames\ncheck track\n").unwrap();
        both.validate(Level::Render, Path::new(".")).unwrap();
    }

    #[test]
    fn a_second_recording_refused_leaves_nothing_of_the_first_to_check() {
        let dir = folder("second-refused");
        let text = "frames 1\ncheck frames\nframes 1\ncheck frames\n";
        let scenario = Scenario::parse(text).unwrap();
        let refused = json!({"ok": false, "error": "not now"});
        let mut fake = Fake::new(&dir, vec![json!({"ok": true}), refused]);
        fake.frames = "1000000 1 2 70 12 true 0 0\n".into();
        let report = run(&dir, true).play(&scenario, &mut fake).unwrap();
        assert!(report.lines[1].passed, "{}", report.text());
        let error = report.lines[3]
            .error
            .as_ref()
            .expect("the first was checked again");
        assert!(error.contains("not recorded"), "{error}");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_recording_still_running_refuses_the_next_and_gives_it_nothing() {
        let dir = folder("second-early");
        let text = "frames 1\ncheck frames\nframes 1\ncheck frames\n";
        let scenario = Scenario::parse(text).unwrap();
        let refused = json!({"ok": false, "error": "frames is still recording"});
        let mut fake = Fake::new(&dir, vec![json!({"ok": true}), refused]);
        fake.finishes = false;
        let report = run(&dir, true).play(&scenario, &mut fake).unwrap();
        assert!(report.lines[1].error.as_ref().unwrap().contains("stopped"));
        assert!(
            report.lines[2]
                .error
                .as_ref()
                .unwrap()
                .contains("still recording")
        );
        let error = report.lines[3]
            .error
            .as_ref()
            .expect("nothing was recorded");
        assert!(error.contains("not recorded"), "{error}");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_condition_on_the_command_line_is_refused_as_in_a_scenario() {
        let refused_by_scenario = |verb: &str, level: Level| {
            Scenario::parse(verb)
                .and_then(|scenario| scenario.validate(level, Path::new(".")))
                .unwrap_err()
        };
        let one_word = "a value is one word";
        for (verb, error) in [
            ("assert chunks >= 200 30000", one_word),
            ("wait_until chunks >= 200 30s", one_word),
            ("assert chat >= 3", "chat is not a number"),
            ("assert screen==", "compares with nothing"),
            ("wait_until chunks >= 200 30000", one_word),
            ("wait_until 30s chunks >= 200", "bad condition 30s chunks"),
        ] {
            for level in ALL {
                let refused = condition::check(verb, *level).unwrap_err();
                assert!(refused.contains(error), "{verb}: {refused}");
                let refused = refused_by_scenario(verb, *level);
                assert!(refused.contains(error), "{verb}: {refused}");
            }
        }
        let limit = control::timeout_of("wait_until chunks >= 200");
        assert_eq!(limit, Duration::from_millis(DEFAULT_WAIT_MS + 5_000));
    }

    #[test]
    fn a_far_jump_across_missing_ticks_reaches_the_report_and_passes() {
        let dir = folder("gap-warning");
        let scenario = Scenario::parse("frames 2\ncheck frames\n").unwrap();
        let mut fake = Fake::new(&dir, vec![]);
        fake.frames = "1000000 0 0 70 12 true 0 0\n1200000 70 0 70 12 true 0 0\n".into();
        let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
        assert!(report.lines[1].passed, "{}", report.text());
        assert!(
            report.text().contains("WARN 70.00 blocks"),
            "{}",
            report.text()
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_second_recording_truncated_leaves_nothing_of_the_first_to_check() {
        let dir = folder("second-short");
        let text = "frames 1\ncheck frames\nframes 5\ncheck frames\ncheck frames\n";
        let scenario = Scenario::parse(text).unwrap();
        let mut fake = Fake::new(&dir, vec![]);
        fake.frames = "1000000 1 2 70 12 true 0 0\n".into();
        let report = run(&dir, true).play(&scenario, &mut fake).unwrap();
        assert!(
            report.lines[3]
                .error
                .as_ref()
                .unwrap()
                .contains("truncated")
        );
        let error = report.lines[4]
            .error
            .as_ref()
            .expect("the first was checked again");
        assert!(error.contains("not recorded"), "{error}");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn what_a_scenario_does_not_play_is_refused_and_the_readme_says_so() {
        let readme = include_str!("../README.md");
        for verb in ["show", "hide", "thumb", "perf", "quit"] {
            let scenario = Scenario::parse(verb).unwrap();
            let error = scenario
                .validate(Level::Render, Path::new("."))
                .unwrap_err();
            assert!(error.contains("unknown verb"), "{error}");
            assert!(readme.contains(&format!("`{verb}`")), "{verb}");
        }
        assert!(!readme.contains("Every verb of the level is there"));
    }

    #[test]
    fn a_camera_that_keeps_moving_fails_and_a_still_one_passes() {
        let dir = folder("still");
        let scenario = Scenario::parse("wait_still 10 200\n").unwrap();
        let mut fake = Fake::new(&dir, vec![]);
        fake.state["cam_x"] = json!(1.0);
        assert!(run(&dir, false).play(&scenario, &mut fake).unwrap().passed);
        fake.moving = true;
        let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
        assert!(
            report.lines[0]
                .error
                .as_ref()
                .unwrap()
                .contains("did not stop")
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_report_is_written_in_both_forms() {
        let dir = folder("report");
        let scenario = Scenario::parse("act map\n").unwrap();
        let mut fake = Fake::new(&dir, vec![]);
        let the_run = run(&dir, false);
        let report = the_run.play(&scenario, &mut fake).unwrap();
        report.write(&the_run.out).unwrap();
        let json: Value =
            serde_json::from_slice(&fs::read(dir.join("out/report.json")).unwrap()).unwrap();
        assert_eq!(json["passed"], true);
        assert_eq!(json["lines"][0]["line"], 1);
        let text = fs::read_to_string(dir.join("out/report.txt")).unwrap();
        assert!(text.contains("PASSED"));
        fs::remove_dir_all(dir).unwrap();
    }

    struct Stage {
        dir: PathBuf,
        clients: BTreeMap<String, (Level, bool)>,
        up: Result<(), String>,
        down: Result<(), String>,
        ups: Vec<String>,
        downs: Vec<String>,
    }

    impl Stage {
        fn new(dir: &Path) -> Self {
            Self {
                dir: dir.to_path_buf(),
                clients: BTreeMap::new(),
                up: Ok(()),
                down: Ok(()),
                ups: Vec::new(),
                downs: Vec::new(),
            }
        }
    }

    impl Host for Stage {
        type Driver = Fake;

        fn find(&self, name: &str) -> Option<(Level, bool)> {
            self.clients.get(name).copied()
        }

        fn up(&mut self, feature: &str, _: Level, _: Option<String>) -> Result<(), String> {
            self.ups.push(feature.to_string());
            self.up.clone()
        }

        fn down(&mut self, name: &str) -> Result<(), String> {
            self.downs.push(name.to_string());
            self.down.clone()
        }

        fn driver(&self, _: &str) -> Fake {
            Fake::new(&self.dir, vec![])
        }
    }

    #[test]
    fn run_refuses_a_probe_of_another_version() {
        let stale = crate::control::stale::Stale::start(Level::Render);
        let dir = registry::home();
        fs::write(dir.join("s.scn"), "act map\n").unwrap();
        let mut host =
            Live(|_: &str, _, _: Option<String>| Err("no client starts here".to_string()));
        stale.assert_refused(command(
            &words(&dir, &["--on", crate::control::stale::NAME]),
            &mut host,
        ));
    }

    fn words(dir: &Path, more: &[&str]) -> Vec<String> {
        let mut args = vec![
            dir.join("s.scn").display().to_string(),
            "--out".to_string(),
            dir.join("out").display().to_string(),
        ];
        args.extend(more.iter().map(|word| word.to_string()));
        args
    }

    fn staged(name: &str, text: &str) -> PathBuf {
        let dir = folder(name);
        fs::write(dir.join("s.scn"), text).unwrap();
        dir
    }

    #[test]
    fn a_run_starts_its_client_stops_it_and_writes_its_report() {
        let dir = staged("command", "act map\n");
        let mut stage = Stage::new(&dir);
        command(&words(&dir, &["--feature", "Carte"]), &mut stage).unwrap();
        assert_eq!((stage.ups.len(), stage.downs.len()), (1, 1));
        assert!(dir.join("out/report.json").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn keep_up_leaves_the_client_and_a_client_of_someone_else_is_never_stopped() {
        let dir = staged("keep-up", "act map\n");
        let mut stage = Stage::new(&dir);
        command(
            &words(&dir, &["--feature", "Carte", "--keep-up"]),
            &mut stage,
        )
        .unwrap();
        assert!(stage.downs.is_empty());
        stage.clients.insert("carte".into(), (Level::Render, true));
        command(&words(&dir, &["--on", "carte"]), &mut stage).unwrap();
        assert!(stage.downs.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_client_that_never_gets_in_game_does_not_stay_alive() {
        let dir = staged("no-join", "act map\n");
        let mut stage = Stage::new(&dir);
        stage.up = Err("not in game after 90 s".into());
        let error = command(&words(&dir, &["--feature", "Carte"]), &mut stage).unwrap_err();
        assert!(error.contains("not in game"));
        assert_eq!(stage.downs, ["carte"]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failed_stop_does_not_lose_the_report() {
        let dir = staged("no-stop", "act map\n");
        let mut stage = Stage::new(&dir);
        stage.down = Err("carte did not stop".into());
        let error = command(&words(&dir, &["--feature", "Carte"]), &mut stage).unwrap_err();
        assert!(error.contains("did not stop"));
        assert!(dir.join("out/report.txt").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failed_line_exits_with_an_error_and_stops_the_client() {
        let dir = staged("failed-line", "frames 500\n");
        let mut stage = Stage::new(&dir);
        let error = command(&words(&dir, &["--feature", "Carte"]), &mut stage).unwrap_err();
        assert!(error.is_empty(), "{error}");
        assert_eq!(stage.downs, ["carte"]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_refused_scenario_starts_no_client() {
        let dir = staged("refused", "level bot\nshot a\n");
        let mut stage = Stage::new(&dir);
        assert!(command(&words(&dir, &["--feature", "Carte"]), &mut stage).is_err());
        assert!(stage.ups.is_empty() && stage.downs.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_target_is_a_running_client_of_the_right_level() {
        let dir = folder("target");
        let mut stage = Stage::new(&dir);
        stage.clients.insert("carte".into(), (Level::Render, true));
        stage.clients.insert("mort".into(), (Level::Render, false));
        stage.clients.insert("jouer".into(), (Level::Play, true));
        let args =
            |line: &str| -> Vec<String> { line.split_whitespace().map(str::to_string).collect() };
        let plain = Scenario::parse("act map\n").unwrap();
        let bot = Scenario::parse("level bot\nact map\n").unwrap();
        let on = target(&args("--on carte"), &plain, &stage).unwrap();
        assert_eq!((on.level, on.feature), (Level::Render, None));
        assert!(target(&args("--on nul"), &plain, &stage).is_err());
        assert!(target(&args("--on mort"), &plain, &stage).is_err());
        assert!(target(&args("--on jouer"), &plain, &stage).is_err());
        assert!(target(&args("--on carte"), &bot, &stage).is_err());
        assert!(target(&args("--on carte --level logic"), &plain, &stage).is_err());
        assert!(target(&[], &plain, &stage).is_err());
        let error = target(&args("--feature Carte"), &plain, &stage)
            .err()
            .unwrap();
        assert!(error.contains("--on carte"), "{error}");
        let fresh = target(&args("--feature Neuve --level logic"), &plain, &stage).unwrap();
        assert_eq!((fresh.name.as_str(), fresh.level), ("neuve", Level::Logic));
        assert!(target(&args("--feature Neuve --level play"), &plain, &stage).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
