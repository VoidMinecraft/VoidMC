use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::compare::Picture;

const TICK_MICROS: u64 = 50_000;
const TICK_MILLIS: u64 = 50;
/// Past this a client's glide outlives the jump and pulls back: see `void-map/src/players.rs`.
const FAR_BLOCKS: f64 = 64.0;
const BACKWARDS: &str = "the time went back, as a probe older than this voidc lets a clock verb move it during a recording";
/// `voidc step` leaves its contact sheet among the frames.
const SHEET: &str = "sheet.png";

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Frame {
    pub t: u64,
    #[serde(default)]
    pub k: u32,
    pub x: f64,
    #[serde(default)]
    pub y: Option<f64>,
    pub z: f64,
    #[serde(default)]
    pub yaw: Option<f64>,
    #[serde(default)]
    pub pitch: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cut {
    from: (u64, Option<u32>),
    to: (u64, Option<u32>),
}

impl Cut {
    /// `56`, `56:2` or `56-60`.
    fn parse(text: &str) -> Result<Self, String> {
        let bad = || format!("bad cut {text}: tick, tick:image or tick-tick");
        if let Some((from, to)) = text.split_once('-') {
            let from: u64 = from.parse().map_err(|_| bad())?;
            let to: u64 = to.parse().map_err(|_| bad())?;
            if to < from {
                return Err(bad());
            }
            return Ok(Self {
                from: (from, None),
                to: (to, None),
            });
        }
        let (tick, image) = match text.split_once(':') {
            Some((tick, image)) => (tick, Some(image.parse().map_err(|_| bad())?)),
            None => (text, None),
        };
        let tick = tick.parse().map_err(|_| bad())?;
        Ok(Self {
            from: (tick, image),
            to: (tick, image),
        })
    }

    fn covers(&self, frame: &Frame) -> bool {
        let after = (frame.t, frame.k) >= (self.from.0, self.from.1.unwrap_or(0));
        let before = (frame.t, frame.k) <= (self.to.0, self.to.1.unwrap_or(u32::MAX));
        after && before
    }

    fn label(&self) -> String {
        let one = |(tick, image): (u64, Option<u32>)| match image {
            Some(image) => format!("{tick}:{image}"),
            None => tick.to_string(),
        };
        if self.from == self.to {
            one(self.from)
        } else {
            format!("{}-{}", one(self.from), one(self.to))
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Limits {
    pub jump: f64,
    pub turn: f64,
    pub flash: f64,
    pub cuts: Vec<Cut>,
    pub images: Option<PathBuf>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            jump: 24.0,
            turn: 120.0,
            flash: 30.0,
            cuts: Vec::new(),
            images: None,
        }
    }
}

impl Limits {
    pub fn parse(words: &[String], base: &Path) -> Result<Self, String> {
        let mut limits = Self::default();
        for word in words {
            let (key, value) = word
                .split_once('=')
                .ok_or_else(|| format!("bad option {word}: key=value"))?;
            let number = || {
                value
                    .parse::<f64>()
                    .ok()
                    .filter(|limit| limit.is_finite() && *limit > 0.0)
                    .ok_or_else(|| format!("bad {word}: a positive number"))
            };
            match key {
                "jump" => limits.jump = number()?,
                "turn" => limits.turn = number()?,
                "flash" => limits.flash = number()?,
                "cut" => limits.cuts.push(Cut::parse(value)?),
                "images" => limits.images = Some(base.join(value)),
                _ => {
                    return Err(format!(
                        "unknown option {key}: jump, turn, flash, cut, images"
                    ));
                }
            }
        }
        Ok(limits)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Finding {
    pub kind: &'static str,
    pub t: u64,
    pub k: u32,
    pub value: f64,
    pub limit: f64,
    pub expected: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub frames: usize,
    pub pictures: usize,
    pub findings: Vec<Finding>,
    pub unused_cuts: Vec<String>,
    pub not_checked: Vec<&'static str>,
    pub warnings: Vec<String>,
    pub passed: bool,
}

impl Outcome {
    pub fn summary(&self) -> String {
        let mut text = format!(
            "{} frames, {} pictures, {} expected cut(s), {} error(s)",
            self.frames,
            self.pictures,
            self.findings.iter().filter(|f| f.expected).count(),
            self.findings.iter().filter(|f| !f.expected).count() + self.unused_cuts.len()
        );
        for finding in &self.findings {
            text.push_str(&format!(
                "\n  {} {} at tick {} image {}: {:.2}, limit {}",
                if finding.expected { "cut " } else { "FAIL" },
                finding.kind,
                finding.t,
                finding.k,
                finding.value,
                finding.limit
            ));
        }
        for cut in &self.unused_cuts {
            text.push_str(&format!(
                "\n  FAIL cut {cut} was declared and nothing happened"
            ));
        }
        if let Some(note) = self.note() {
            text.push_str(&format!("\n  {}", note.replace('\n', "\n  ")));
        }
        text
    }

    /// What a passing check still has to say.
    pub fn note(&self) -> Option<String> {
        let mut lines: Vec<String> = self
            .warnings
            .iter()
            .map(|warning| format!("WARN {warning}"))
            .collect();
        if !self.not_checked.is_empty() {
            lines.push(format!("not checked: {}", self.not_checked.join(", ")));
        }
        (!lines.is_empty()).then(|| lines.join("\n"))
    }
}

/// Reads the JSONL of a stepped run, or the `frames.txt` and `track.txt` of the verbs of that
/// name, which know the time and not the tick.
pub fn parse_frames(text: &str) -> Result<Vec<Frame>, String> {
    let lines: Vec<(usize, &str)> = text
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| (index + 1, line))
        .collect();
    let Some((_, first)) = lines.first() else {
        return Err("no frames".into());
    };
    if first.trim_start().starts_with('{') {
        return lines
            .iter()
            .map(|(number, line)| {
                serde_json::from_str(line).map_err(|e| format!("line {number}: {e}"))
            })
            .collect();
    }
    let rows: Vec<(usize, Vec<f64>)> = lines
        .iter()
        .map(|(number, line)| row_of(*number, line))
        .collect::<Result<_, _>>()?;
    let start = rows[0].1.first().copied().unwrap_or(0.0);
    let mut frames: Vec<Frame> = Vec::with_capacity(rows.len());
    let mut previous = start;
    for (number, row) in &rows {
        let time = row.first().copied().unwrap_or(f64::NAN);
        if time < previous {
            return Err(format!(
                "line {number}: the clock went from {previous} to {time}: {BACKWARDS}"
            ));
        }
        previous = time;
        let (tick, frame) = match row[..] {
            [time, x, z, y, _, _, yaw, pitch] => (
                elapsed(time, start, *number)? / TICK_MICROS,
                Frame {
                    t: 0,
                    k: 0,
                    x,
                    y: Some(y),
                    z,
                    yaw: Some(yaw),
                    pitch: Some(pitch),
                },
            ),
            [time, x, z] => (
                elapsed(time, start, *number)? / TICK_MILLIS,
                Frame {
                    t: 0,
                    k: 0,
                    x,
                    y: None,
                    z,
                    yaw: None,
                    pitch: None,
                },
            ),
            _ => return Err(format!("line {number}: neither frames.txt nor track.txt")),
        };
        let image = frames
            .last()
            .filter(|last| last.t == tick)
            .map_or(0, |last| last.k + 1);
        frames.push(Frame {
            t: tick,
            k: image,
            ..frame
        });
    }
    Ok(frames)
}

fn row_of(number: usize, line: &str) -> Result<(usize, Vec<f64>), String> {
    line.split_whitespace()
        .map(|word| match word {
            "true" => Ok(1.0),
            "false" => Ok(0.0),
            _ => word.parse(),
        })
        .collect::<Result<Vec<f64>, _>>()
        .map(|row| (number, row))
        .map_err(|e| format!("line {number}: {e}"))
}

fn elapsed(time: f64, start: f64, number: usize) -> Result<u64, String> {
    if time.is_finite() && time >= start {
        Ok((time - start) as u64)
    } else {
        Err(format!(
            "line {number}: the clock went from {start} to {time}: {BACKWARDS}"
        ))
    }
}

fn finite(frame: &Frame) -> bool {
    [frame.y, frame.yaw, frame.pitch]
        .into_iter()
        .flatten()
        .chain([frame.x, frame.z])
        .all(f64::is_finite)
}

fn distance(a: &Frame, b: &Frame) -> f64 {
    let height = match (a.y, b.y) {
        (Some(a), Some(b)) => a - b,
        _ => 0.0,
    };
    ((a.x - b.x).powi(2) + height.powi(2) + (a.z - b.z).powi(2)).sqrt()
}

/// The turn from one angle to another, the short way, whatever the turns before them.
fn turned(from: f64, to: f64) -> f64 {
    let turn = (to - from).rem_euclid(360.0);
    turn.min(360.0 - turn)
}

fn numbers_in(name: &str) -> Vec<u64> {
    name.split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse().ok())
        .collect()
}

fn picture_index(stem: &str) -> Option<u64> {
    let digits = stem.strip_prefix('f')?;
    if digits.len() < 5 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// A stepped run names its pictures `f00241.png`, by `t * K + k` with K images in a tick.
fn paired_by_index(
    files: &[PathBuf],
    frames: &[Frame],
    stem: impl Fn(&Path) -> String,
) -> Option<Vec<(usize, PathBuf)>> {
    let per_tick = u64::from(frames.iter().map(|frame| frame.k).max()?) + 1;
    let by_index: BTreeMap<u64, usize> = frames
        .iter()
        .enumerate()
        .map(|(index, frame)| (frame.t * per_tick + u64::from(frame.k), index))
        .collect();
    let mut paired = Vec::with_capacity(files.len());
    for file in files {
        let number = picture_index(&stem(file))?;
        paired.push((*by_index.get(&number)?, file.clone()));
    }
    paired.sort();
    Some(paired)
}

/// Pairs the pictures with the frames: by `<tick>-<image>` or `f<index>` in the name, or one
/// for each frame.
fn pictures(dir: &Path, frames: &[Frame]) -> Result<Vec<(usize, PathBuf)>, String> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "png"))
        .filter(|path| path.file_name().is_none_or(|name| name != SHEET))
        .collect();
    files.sort();
    let stem = |path: &Path| {
        path.file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    if !files.is_empty() && files.iter().all(|file| numbers_in(&stem(file)).len() == 2) {
        let by_frame: BTreeMap<(u64, u32), usize> = frames
            .iter()
            .enumerate()
            .map(|(index, frame)| ((frame.t, frame.k), index))
            .collect();
        let mut paired = Vec::new();
        for file in files {
            let numbers = numbers_in(&stem(&file));
            let at = (numbers[0], numbers[1] as u32);
            let index = by_frame
                .get(&at)
                .ok_or_else(|| format!("{} has no frame", file.display()))?;
            paired.push((*index, file));
        }
        paired.sort();
        return Ok(paired);
    }
    if let Some(paired) = paired_by_index(&files, frames, stem) {
        return Ok(paired);
    }
    if files.len() != frames.len() {
        return Err(format!(
            "{} pictures for {} frames in {}: name them <tick>-<image>.png",
            files.len(),
            frames.len(),
            dir.display()
        ));
    }
    Ok(files.into_iter().enumerate().collect())
}

/// Each change of brightness, as the frame it lands on and how far it went.
fn flashes(dir: &Path, frames: &[Frame]) -> Result<Vec<(usize, f64)>, String> {
    let paired = pictures(dir, frames)?;
    let mut levels = Vec::with_capacity(paired.len());
    for (index, file) in paired {
        levels.push((index, Picture::load(&file)?.brightness()));
    }
    Ok(levels
        .windows(2)
        .map(|pair| (pair[1].0, (pair[1].1 - pair[0].1).abs()))
        .collect())
}

/// What a pair of frames, or of pictures, did past a limit: where, what, how far, and the limit.
type Raw = (usize, &'static str, f64, f64);

fn refuse_unreadable(frames: &[Frame]) -> Result<(), String> {
    if frames.is_empty() {
        return Err("no frames".into());
    }
    if let Some(frame) = frames.iter().find(|frame| !finite(frame)) {
        return Err(format!(
            "tick {} image {} is not a number: {frame:?}",
            frame.t, frame.k
        ));
    }
    if let Some(pair) = frames
        .windows(2)
        .find(|pair| (pair[1].t, pair[1].k) <= (pair[0].t, pair[0].k))
    {
        return Err(format!(
            "frames out of order at tick {} image {}",
            pair[1].t, pair[1].k
        ));
    }
    Ok(())
}

/// Jumps and half turns, and the far jumps across missing ticks that only warn.
fn motion(frames: &[Frame], limits: &Limits) -> (Vec<Raw>, Vec<String>) {
    let mut raw = Vec::new();
    let mut warnings = Vec::new();
    for (index, pair) in frames.windows(2).enumerate() {
        let (from, to) = (&pair[0], &pair[1]);
        let at = index + 1;
        let step = distance(from, to);
        let ticks = (to.t - from.t).max(1) as f64;
        let allowed = limits.jump * ticks;
        let far = FAR_BLOCKS * ticks;
        if step > far {
            raw.push((at, "jump past 64 blocks", step, far));
        } else if step > allowed {
            raw.push((at, "camera jump", step, allowed));
        } else if ticks > 1.0 && step > FAR_BLOCKS {
            warnings.push(format!(
                "{step:.2} blocks in a gap of {ticks} ticks before tick {}: within the limit by time, past {FAR_BLOCKS} blocks at once",
                to.t
            ));
        }
        let yaw = from.yaw.zip(to.yaw).map(|(a, b)| turned(a, b));
        let pitch = from.pitch.zip(to.pitch).map(|(a, b)| (a - b).abs());
        for turn in [yaw, pitch].into_iter().flatten() {
            if turn >= limits.turn {
                raw.push((at, "half turn", turn, limits.turn));
            }
        }
    }
    (raw, warnings)
}

/// Each finding, expected when a cut covers it, and the cuts that covered nothing.
fn judge(mut raw: Vec<Raw>, frames: &[Frame], cuts: &[Cut]) -> (Vec<Finding>, Vec<String>) {
    raw.sort_by_key(|finding| finding.0);
    let mut used = vec![false; cuts.len()];
    let findings = raw
        .into_iter()
        .map(|(index, kind, value, limit)| {
            let frame = &frames[index];
            let mut expected = false;
            for (cut, used) in cuts.iter().zip(&mut used) {
                if cut.covers(frame) {
                    *used = true;
                    expected = true;
                }
            }
            Finding {
                kind,
                t: frame.t,
                k: frame.k,
                value,
                limit,
                expected,
            }
        })
        .collect();
    let unused = cuts
        .iter()
        .zip(&used)
        .filter(|(_, used)| !**used)
        .map(|(cut, _)| cut.label())
        .collect();
    (findings, unused)
}

fn not_checked(frames: &[Frame], limits: &Limits) -> Vec<&'static str> {
    let mut not_checked = Vec::new();
    if frames.iter().all(|frame| frame.yaw.is_none()) {
        not_checked.push("half turns: these frames have no angles");
    }
    if frames.iter().all(|frame| frame.y.is_none()) {
        not_checked.push("height: these frames have none");
    }
    if limits.images.is_none() {
        not_checked.push("flashes: no images=");
    }
    not_checked
}

pub fn check(frames: &[Frame], limits: &Limits) -> Result<Outcome, String> {
    refuse_unreadable(frames)?;
    let (mut raw, warnings) = motion(frames, limits);
    let mut pictures_seen = 0;
    if let Some(dir) = &limits.images {
        let changes = flashes(dir, frames)?;
        pictures_seen = changes.len() + 1;
        for (index, change) in changes {
            if change >= limits.flash {
                raw.push((index, "flash", change, limits.flash));
            }
        }
    }
    let (findings, unused_cuts) = judge(raw, frames, &limits.cuts);
    let passed = unused_cuts.is_empty() && findings.iter().all(|finding| finding.expected);
    Ok(Outcome {
        frames: frames.len(),
        pictures: pictures_seen,
        findings,
        unused_cuts,
        not_checked: not_checked(frames, limits),
        warnings,
        passed,
    })
}

pub fn read(path: &Path) -> Result<String, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let invalid = text
        .lines()
        .enumerate()
        .find_map(|(index, line)| Some((index + 1, line.trim().strip_prefix("invalid: ")?)));
    match invalid {
        Some((number, reason)) => Err(format!(
            "{} is invalid, line {number}: {reason}",
            path.display()
        )),
        None => Ok(text),
    }
}

pub fn file(path: &Path, limits: &Limits) -> Result<Outcome, String> {
    let text = read(path)?;
    let mut outcome = check(&parse_frames(&text)?, limits)?;
    outcome.warnings.extend(frozen(&text));
    Ok(outcome)
}

/// Lines of `frames.txt` or `track.txt` that all carry one time came under a frozen clock:
/// nothing moved between them, and nothing could be found.
fn frozen(text: &str) -> Option<String> {
    let mut times = text
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|time| !time.starts_with('{'));
    let first = times.next()?;
    let mut count = 1;
    for time in times {
        if time != first {
            return None;
        }
        count += 1;
    }
    (count > 1).then(|| {
        format!("all {count} frames fall at the same instant: the clock was virtual and nothing stepped it")
    })
}

/// `voidc check <frames> [cut=56] [jump=N] [turn=N] [flash=N] [images=dir] [--json out.json]`.
pub fn command(args: &[String]) -> Result<(), String> {
    let [path, rest @ ..] = args else {
        return Err("voidc check <frames> [cut=T|T:K|T-T] [jump=N] [turn=N] [flash=N] [images=dir] [--json out.json]".into());
    };
    let (mut words, mut json) = (Vec::new(), None);
    let mut rest = rest.iter();
    while let Some(word) = rest.next() {
        if word == "--json" {
            json = rest.next().map(PathBuf::from);
        } else {
            words.push(word.clone());
        }
    }
    let outcome = file(Path::new(path), &Limits::parse(&words, Path::new("."))?)?;
    println!("{}", outcome.summary());
    if let Some(json) = json {
        let text = serde_json::to_string_pretty(&outcome).map_err(|e| e.to_string())?;
        fs::write(&json, text).map_err(|e| format!("{}: {e}", json.display()))?;
    }
    if outcome.passed {
        Ok(())
    } else {
        Err("the animation does not hold".into())
    }
}

#[cfg(test)]
mod tests;
