use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::control::{self, Ask, Chore, Limits, Probe};
use crate::registry::{self, Client, Level, Registry};
use crate::{args, lifecycle};

const TICK_MS: f64 = 50.0;
const TICKS_PER_SECOND: u64 = 20;
const STEP_FPS: u32 = 260;
const SERVER_LIMIT: Duration = Duration::from_secs(120);
const QUIET_POLLS: u32 = 4;
const QUIET_LIMIT: u32 = 200;
const QUIET_POLL: Duration = Duration::from_millis(50);
const SETTLE_TICKS: u64 = 20;
const MIN_CHUNKS: u64 = 200;
const SPARSE_VIDEO_RATE: u64 = 4;
const FRAMES_GLOB: &str = "f[0-9]*.png";
const LOCK_TRIES: u32 = 3;
const SHEET_COLUMNS: u64 = 6;
const SHEET_CELLS: u64 = 24;

fn counter(reply: &Value, name: &str) -> Result<u64, String> {
    reply[name]
        .as_u64()
        .ok_or_else(|| format!("no {name} in {reply}"))
}

/// The two counters of each side, taken from the same quiet moment.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Origin {
    sent: u64,
    received: u64,
    client_in: u64,
    client_out: u64,
}

struct Lockstep<S, C> {
    server: S,
    client: C,
    origin: Origin,
}

impl<S: Ask, C: Ask> Lockstep<S, C> {
    fn counters(&mut self) -> Result<(u64, u64), String> {
        let reply = self.client.ask("sent")?;
        if reply["active"] != true {
            return Err("the client lost its connection to the server".into());
        }
        Ok((counter(&reply, "in")?, counter(&reply, "out")?))
    }

    /// The server is still, so counters that stop moving have nothing left in flight.
    fn calibrate(&mut self, poll: Duration) -> Result<(), String> {
        let mut last = None;
        let mut quiet = 0;
        for _ in 0..QUIET_LIMIT {
            let status = self.server.ask("status")?;
            let (client_in, client_out) = self.counters()?;
            let origin = Origin {
                sent: counter(&status, "sent")?,
                received: counter(&status, "received")?,
                client_in,
                client_out,
            };
            quiet = if last == Some(origin) { quiet + 1 } else { 0 };
            last = Some(origin);
            if quiet >= QUIET_POLLS {
                self.origin = origin;
                return Ok(());
            }
            sleep(poll);
        }
        Err("the counters never settled: is another client on this server?".into())
    }

    /// One tick, with everything the client wrote before it and everything it sent after it.
    fn tick(&mut self) -> Result<u64, String> {
        let (_, client_out) = self.counters()?;
        let need = self.origin.received + client_out - self.origin.client_out;
        let reply = self.server.ask(&format!("step 1 {need}"))?;
        let sent = counter(&reply, "sent")?;
        let want = self.origin.client_in + sent - self.origin.sent;
        self.client.ask(&format!("fence {want}"))?;
        counter(&reply, "tick")
    }
}

#[derive(Debug, PartialEq)]
struct Plan {
    server: PathBuf,
    out: PathBuf,
    ticks: u64,
    frames: u64,
    lock_at: u64,
    min_chunks: u64,
    acts: Vec<String>,
    every: u64,
    at: Vec<u64>,
    hold: Option<(u64, u64)>,
    stop_server: bool,
}

impl Plan {
    fn parse(args: &[String], level: Level) -> Result<Self, String> {
        let at = args::flag(args, "--at")
            .map(|list| {
                list.split(',')
                    .map(|index| index.parse().map_err(|_| "bad --at".to_string()))
                    .collect::<Result<Vec<u64>, String>>()
            })
            .transpose()?
            .unwrap_or_default();
        let every = args::value(args, "--every", if at.is_empty() { 1 } else { 0 })?;
        let hold = args::flag(args, "--hold")
            .map(|text| {
                let (tick, seconds) = text.split_once(':').ok_or("--hold is tick:seconds")?;
                Ok::<_, String>((
                    tick.parse().map_err(|_| "bad --hold tick")?,
                    seconds.parse().map_err(|_| "bad --hold seconds")?,
                ))
            })
            .transpose()?;
        let plan = Self {
            server: args::flag(args, "--server")
                .ok_or("--server: the step socket of the server")?
                .into(),
            out: args::flag(args, "--out")
                .ok_or("--out: where to write")?
                .into(),
            ticks: args::value(args, "--ticks", 120)?,
            frames: args::value(args, "--frames", 4)?,
            lock_at: args::value(args, "--lock-at", 600)?,
            min_chunks: args::value(args, "--min-chunks", MIN_CHUNKS)?,
            acts: args::all(args, "--act"),
            every,
            at,
            hold,
            stop_server: args::has(args, "--stop-server"),
        };
        if plan.frames == 0 {
            return Err("--frames is at least 1".into());
        }
        if level == Level::Logic && plan.captures() {
            return Err("the logic level has no picture: pass --every 0".into());
        }
        if level != Level::Render && level != Level::Logic {
            return Err("voidc step takes --level render or logic".into());
        }
        Ok(plan)
    }

    fn captures(&self) -> bool {
        self.every > 0 || !self.at.is_empty()
    }

    fn wanted(&self, index: u64) -> bool {
        (self.every > 0 && index.is_multiple_of(self.every)) || self.at.contains(&index)
    }

    /// Real speed when pictures are evenly spaced. Picked by hand, they are slow enough to look at.
    fn video_rate(&self) -> u64 {
        if !self.at.is_empty() {
            return SPARSE_VIDEO_RATE;
        }
        (TICKS_PER_SECOND * self.frames / self.every.max(1)).max(1)
    }
}

fn measure(t: u64, k: u64, camera: &Value) -> String {
    format!(
        "{{\"t\":{t},\"k\":{k},\"x\":{},\"y\":{},\"z\":{},\"yaw\":{},\"pitch\":{}}}",
        camera["x"], camera["y"], camera["z"], camera["yaw"], camera["pitch"]
    )
}

fn frame_name(index: u64) -> String {
    format!("f{index:05}")
}

fn play<S: Ask, C: Ask>(
    lock: &mut Lockstep<S, C>,
    plan: &Plan,
    measures: &mut impl Write,
    mut keep: impl FnMut(&str, &Path) -> Result<(), String>,
) -> Result<u64, String> {
    let frame_ms = TICK_MS / plan.frames as f64;
    let mut captured = 0;
    for t in 0..plan.ticks {
        lock.tick()
            .map_err(|error| format!("tick {t} of the passage: {error}"))?;
        for k in 0..plan.frames {
            let camera = lock.client.ask(&format!("step {frame_ms}"))?;
            writeln!(measures, "{}", measure(t, k, &camera)).map_err(|e| e.to_string())?;
            let index = t * plan.frames + k;
            if plan.wanted(index) {
                let name = frame_name(index);
                let shot = lock.client.ask(&format!("shot {name}"))?;
                let path = shot["path"].as_str().ok_or("shot without a path")?;
                keep(&name, Path::new(path))?;
                captured += 1;
            }
        }
        if plan.hold.is_some_and(|(tick, _)| tick == t) {
            let seconds = plan.hold.map_or(0, |(_, seconds)| seconds);
            println!("holding {seconds} s at tick {t}");
            sleep(Duration::from_secs(seconds));
        }
    }
    lock.counters()
        .map_err(|error| format!("end of the passage: {error}"))?;
    Ok(captured)
}

fn settle<S: Ask, C: Ask>(lock: &mut Lockstep<S, C>) -> Result<(), String> {
    lock.client.ask("clock virtual")?;
    lock.calibrate(QUIET_POLL)?;
    for _ in 0..SETTLE_TICKS {
        lock.tick()?;
        lock.client.ask(&format!("step {TICK_MS}"))?;
    }
    Ok(())
}

fn wait_for_server(path: &Path) -> Result<Probe, String> {
    let start = Instant::now();
    loop {
        match UnixStream::connect(path) {
            Ok(stream) => return Probe::over("server", stream, Limits::REAL),
            Err(error) if start.elapsed() > SERVER_LIMIT => {
                return Err(format!("no stepped server on {}: {error}", path.display()));
            }
            Err(_) => sleep(Duration::from_millis(100)),
        }
    }
}

/// What the wait for the lock needs to know about the client.
trait Joining {
    fn alive(&self) -> bool;
    fn in_game(&self) -> bool;
    fn name(&self) -> &str;
    fn min_chunks(&self) -> u64;
}

struct Joiner<'a> {
    client: &'a Client,
    min_chunks: u64,
}

impl Joining for Joiner<'_> {
    fn alive(&self) -> bool {
        registry::alive(self.client)
    }

    fn in_game(&self) -> bool {
        control::answered(control::chore(
            &self.client.name,
            Chore::State,
            Duration::from_millis(500),
        ))
        .ok()
        .is_some_and(|state| {
            state["state"] == "connected" && state["chunks"].as_u64() >= Some(self.min_chunks)
        })
    }

    fn name(&self) -> &str {
        &self.client.name
    }

    fn min_chunks(&self) -> u64 {
        self.min_chunks
    }
}

/// Runs the server at game speed up to the tick of the lock, with the client in game by then.
fn join(
    server: &mut impl Ask,
    client: &impl Joining,
    lock_at: u64,
    pace: Duration,
) -> Result<(), String> {
    let mut tick = counter(&server.ask("status")?, "tick")?;
    if tick >= lock_at {
        return Err(format!(
            "the server is at tick {tick}, past --lock-at {lock_at}: restart it"
        ));
    }
    let mut joined = false;
    while tick < lock_at {
        let next = counter(&server.ask("step 1")?, "tick")?;
        if next <= tick {
            return Err(format!(
                "the server stays at tick {tick}: it does not count"
            ));
        }
        tick = next;
        if !client.alive() {
            return Err(format!("client {} stopped before the lock", client.name()));
        }
        if !joined && tick.is_multiple_of(10) && client.in_game() {
            joined = true;
            println!("{}: in game at tick {tick}", client.name());
        }
        sleep(pace);
    }
    if (0..LOCK_TRIES).any(|_| client.in_game()) {
        return Ok(());
    }
    Err(format!(
        "{} is not in game with {} chunks at tick {lock_at}: raise --lock-at, lower --min-chunks, or see {}",
        client.name(),
        client.min_chunks(),
        registry::client_dir(client.name())
            .join("client.log")
            .display()
    ))
}

/// In the folder itself: its path could hold what a glob reads as a pattern.
fn ffmpeg(out: &Path, args: &[String], what: &str) -> Result<(), String> {
    let output = Command::new("ffmpeg")
        .current_dir(out)
        .args(["-y", "-loglevel", "error"])
        .args(args)
        .output()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "{what}: {}",
        String::from_utf8_lossy(&output.stderr)
    ))
}

/// Picks evenly among `count` pictures, at most a full sheet.
fn sheet_filter(count: u64) -> String {
    let stride = count.div_ceil(SHEET_CELLS).max(1);
    let cells = count.div_ceil(stride);
    let columns = cells.min(SHEET_COLUMNS);
    let rows = cells.div_ceil(columns);
    format!("select='not(mod(n\\,{stride}))',scale=320:-2,tile={columns}x{rows}")
}

fn assemble(out: &Path, rate: u64, count: u64) -> Result<(), String> {
    let pattern = FRAMES_GLOB.to_string();
    let glob = ["-pattern_type".to_string(), "glob".into()];
    let mut video: Vec<String> = glob.to_vec();
    video.extend([
        "-framerate".to_string(),
        rate.to_string(),
        "-i".into(),
        pattern.clone(),
        "-vf".into(),
        "scale=trunc(iw/2)*2:trunc(ih/2)*2".into(),
        "-c:v".into(),
        "libx264".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "video.mp4".into(),
    ]);
    ffmpeg(out, &video, "video")?;
    let mut sheet: Vec<String> = glob.to_vec();
    sheet.extend([
        "-i".to_string(),
        pattern,
        "-vf".into(),
        sheet_filter(count),
        "-fps_mode".into(),
        "passthrough".into(),
        "-frames:v".into(),
        "1".into(),
        "sheet.png".into(),
    ]);
    ffmpeg(out, &sheet, "sheet")
}

/// The same files as `FRAMES_GLOB`, so that nothing left behind can enter a video.
fn is_output(name: &str) -> bool {
    let frame = name
        .strip_prefix('f')
        .and_then(|rest| rest.strip_suffix(".png"))
        .is_some_and(|index| index.starts_with(|c: char| c.is_ascii_digit()));
    frame || matches!(name, "video.mp4" | "sheet.png" | "frames.jsonl")
}

/// A run writes its own files only: what an earlier run left would end up in the video.
fn prepare(out: &Path) -> Result<(), String> {
    fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    for entry in fs::read_dir(out).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if is_output(&entry.file_name().to_string_lossy()) {
            fs::remove_file(entry.path()).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn size_of(out: &Path) -> u64 {
    fs::read_dir(out)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with('f'))
        .filter_map(|entry| entry.metadata().ok())
        .map(|metadata| metadata.len())
        .sum()
}

fn passage(server: &mut impl Ask, client: &Client, plan: &Plan) -> Result<(), String> {
    let joiner = Joiner {
        client,
        min_chunks: plan.min_chunks,
    };
    let pace = Duration::from_millis(1000 / TICKS_PER_SECOND);
    join(server, &joiner, plan.lock_at, pace)?;
    let mut lock = Lockstep {
        server,
        client: Probe::open(&client.name, Limits::REAL)?,
        origin: Origin::default(),
    };
    settle(&mut lock)?;
    for command in &plan.acts {
        lock.client.ask(&format!("act {command}"))?;
    }
    let mut measures =
        BufWriter::new(File::create(plan.out.join("frames.jsonl")).map_err(|e| e.to_string())?);
    let start = Instant::now();
    let captured = play(&mut lock, plan, &mut measures, |name, path| {
        let target = plan.out.join(name).with_extension("png");
        fs::rename(path, &target)
            .or_else(|_| fs::copy(path, &target).and_then(|_| fs::remove_file(path)))
            .map_err(|e| format!("{}: {e}", target.display()))
    })?;
    measures.flush().map_err(|e| e.to_string())?;
    let seconds = start.elapsed().as_secs_f64();
    let total = plan.ticks * plan.frames;
    println!(
        "{total} frames, {captured} captured, {seconds:.1} s, {:.1} frames/s, {:.1} MB",
        total as f64 / seconds,
        size_of(&plan.out) as f64 / 1e6
    );
    if captured > 1 {
        let assembly = Instant::now();
        assemble(&plan.out, plan.video_rate(), captured)?;
        println!(
            "video and sheet in {:.1} s: {}",
            assembly.elapsed().as_secs_f64(),
            plan.out.display()
        );
    }
    Ok(())
}

/// A server left behind would hold its port and its socket against the next run.
fn finish(server: &mut impl Ask, stop: bool, result: Result<(), String>) -> Result<(), String> {
    if !stop {
        return result;
    }
    let stopped = server.ask("stop").map(|_| ()).map_err(String::from);
    result.and(stopped)
}

pub fn run(registry: &Registry, mut client: Client, args: &[String]) -> Result<(), String> {
    let plan = Plan::parse(args, client.level)?;
    client.fps = client.fps.or(Some(STEP_FPS));
    let name = client.name.clone();
    let mut server = wait_for_server(&plan.server)?;
    let started = prepare(&plan.out).and_then(|()| lifecycle::start(registry, client));
    let result = started.and_then(|()| {
        let loaded = registry::load(&name).ok_or("the client was not saved")?;
        let played = passage(&mut server, &loaded, &plan);
        let stopped = lifecycle::down(&name);
        played.and(stopped)
    });
    finish(&mut server, plan.stop_server, result)
}

#[cfg(test)]
mod tests;
