use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::parse::{Line, Scenario, Written};
use super::report::{LineResult, Report, Step};
use crate::condition::DEFAULT_WAIT_MS;
use crate::control::{self, Ask, Chore, Failure, Limits, Probe};
use crate::registry::{self, Level};
use crate::{check, compare, version};

const RECORDING_PATIENCE: Duration = Duration::from_secs(5);
const TRACK_SAMPLE: Duration = Duration::from_millis(5);
const STILL_POLL: Duration = Duration::from_millis(25);
const CAMERA_FIELDS: [&str; 5] = ["cam_x", "cam_y", "cam_z", "cam_yaw", "cam_pitch"];

/// How a scenario sends a condition: as written, and a `wait_until` always carries its limit.
pub(super) fn verb_of(name: &str, condition: &Written, limit: u64) -> String {
    if name == "assert" {
        format!("assert {}", condition.text)
    } else {
        format!("wait_until {limit} {}", condition.text)
    }
}

/// A recording that was started, and the line that started it.
pub(super) struct Recording {
    pub(super) line: usize,
    pub(super) text: String,
    pub(super) lines: Option<usize>,
    pub(super) patience: Duration,
}

/// What this run has made. Nothing else is read, even if `--out` holds a file of that name.
#[derive(Default)]
pub(super) struct Made {
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
    pub(super) level: Level,
    base: PathBuf,
    pub(super) out: PathBuf,
    keep_going: bool,
    pub(super) patience: Duration,
    made: RefCell<Made>,
}

/// The one thing a scenario talks to, so its runs can be tested without a game.
pub trait Driver: Ask {
    /// `clock real` at the end of a run, which nothing may stop on its way to the client.
    fn give_back_clock(&mut self) -> Result<Value, Failure>;
    fn recording(&self, name: &str) -> PathBuf;

    fn read(&self, path: &Path) -> Result<String, String> {
        check::read(path)
    }
}

/// The client of a run, on one checked connection that a lost line takes down: the next verb
/// opens and checks another.
pub struct LiveClient {
    name: String,
    probe: Probe,
    limits: Limits,
}

impl LiveClient {
    pub fn open(name: &str, limits: Limits) -> Result<Self, String> {
        Ok(Self {
            name: name.to_string(),
            probe: Probe::open(name, limits)?,
            limits,
        })
    }
}

impl Ask for LiveClient {
    fn ask(&mut self, verb: &str) -> Result<Value, Failure> {
        self.probe.ask(verb)
    }
}

impl Driver for LiveClient {
    /// On a connection of its own: no check can stop it, and no late answer can reach it.
    fn give_back_clock(&mut self) -> Result<Value, Failure> {
        control::chore(&self.name, Chore::ClockReal, self.limits.of("clock real"))
    }

    fn recording(&self, name: &str) -> PathBuf {
        registry::client_dir(&self.name).join(format!("{name}.txt"))
    }
}

/// Counts whole lines only: the last one may be half written.
pub(super) fn line_count(path: &Path) -> usize {
    fs::read(path).map_or(0, |bytes| {
        bytes.iter().filter(|byte| **byte == b'\n').count()
    })
}

/// `frames` and `track` answer at once and write as they go, then leave a `.done` mark.
pub(super) fn wait_for_recording(
    verb: &str,
    path: &Path,
    recording: &Recording,
) -> Result<(), String> {
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
        seen = match control::answered(driver.ask("state")) {
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
    let error = match driver.ask(&verb_of(&line.verb, condition, limit)) {
        Ok(_) => return Step::default(),
        Err(Failure::Lost(error)) => return Step::failed(error),
        Err(refused) => refused.to_string(),
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
        let reply = match driver.ask(&loaded) {
            Err(Failure::Refused(reply)) => Err(format!(
                "the loading screen stayed: {}",
                Failure::Refused(reply)
            )),
            Err(lost) => Err(lost.to_string()),
            Ok(_) => driver
                .ask(&format!("shot {}", line.words.join(" ")))
                .map_err(String::from),
        };
        let reply = match reply {
            Ok(reply) => reply,
            Err(error) => return Step::failed(error),
        };
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
                detail: Some(reply),
                ..Step::default()
            },
            Err(Failure::Refused(reply)) => Step {
                error: Some(Failure::Refused(reply.clone()).to_string()),
                detail: Some(reply),
                ..Step::default()
            },
            Err(Failure::Lost(error)) => Step::failed(error),
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
        let state = control::answered(driver.ask("state"))?;
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
    let by_command = || {
        format!(
            "Send voidc cmd {client} clock real, or restart the client if voidc refuses its version"
        )
    };
    let (error, advice) = match driver.give_back_clock() {
        Ok(_) => return None,
        Err(Failure::Refused(reply)) => {
            let advice = version::check(client, &reply).map_or_else(
                |_| format!("Its probe takes no voidc cmd: restart the client with voidc down {client}, then voidc up"),
                |()| by_command(),
            );
            (Failure::Refused(reply).to_string(), advice)
        }
        Err(Failure::Lost(error)) => (error, by_command()),
    };
    let result = LineResult::new(
        0,
        "clock real, at the end of the run".to_string(),
        0,
        Step::failed(format!(
            "the clock of {client} was not given back, and the next run could play frozen: {error}. {advice}"
        )),
    );
    println!("{}", result.report());
    Some(result)
}
