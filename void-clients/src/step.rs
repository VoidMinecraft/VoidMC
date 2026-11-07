use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, ErrorKind, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::control::{self, Chore};
use crate::lifecycle;
use crate::registry::{self, Client, Level, Registry};

const TICK_MS: f64 = 50.0;
const TICKS_PER_SECOND: u64 = 20;
const STEP_FPS: u32 = 260;
const SERVER_LIMIT: Duration = Duration::from_secs(120);
const ANSWER_LIMIT: Duration = Duration::from_secs(30);
const QUIET_POLLS: u32 = 4;
const QUIET_LIMIT: u32 = 200;
const SETTLE_TICKS: u64 = 20;
const MIN_CHUNKS: u64 = 200;
const SPARSE_VIDEO_RATE: u64 = 4;
const FRAMES_GLOB: &str = "f[0-9]*.png";
const LOCK_TRIES: u32 = 3;
const SHEET_COLUMNS: u64 = 6;
const SHEET_CELLS: u64 = 24;

/// One line out, one JSON line back: the server's step socket and the client's verbs.
trait Link {
    fn ask(&mut self, line: &str) -> Result<Value, String>;
}

impl<T: Link> Link for &mut T {
    fn ask(&mut self, line: &str) -> Result<Value, String> {
        (**self).ask(line)
    }
}

struct Socket {
    who: String,
    limit: Duration,
    stream: UnixStream,
    reader: BufReader<UnixStream>,
}

impl Socket {
    fn open(who: &str, path: &Path) -> Result<Self, String> {
        let stream = UnixStream::connect(path).map_err(|e| format!("{who}: {e}"))?;
        Self::over(who, stream, ANSWER_LIMIT)
    }

    fn over(who: &str, stream: UnixStream, limit: Duration) -> Result<Self, String> {
        stream
            .set_read_timeout(Some(limit))
            .map_err(|e| e.to_string())?;
        let reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
        Ok(Self {
            who: who.to_string(),
            limit,
            stream,
            reader,
        })
    }
}

impl Link for Socket {
    fn ask(&mut self, line: &str) -> Result<Value, String> {
        writeln!(self.stream, "{line}")
            .map_err(|e| format!("{} stopped answering: {e}", self.who))?;
        let mut answer = String::new();
        self.reader
            .read_line(&mut answer)
            .map_err(|e| match e.kind() {
                ErrorKind::WouldBlock | ErrorKind::TimedOut => format!(
                    "{} gave no answer to {line} in {} s",
                    self.who,
                    self.limit.as_secs_f32()
                ),
                _ => format!("{}: {line}: {e}", self.who),
            })?;
        if answer.is_empty() {
            return Err(format!("{} closed the connection on {line}", self.who));
        }
        let reply: Value = serde_json::from_str(&answer).map_err(|e| e.to_string())?;
        if reply["ok"] == true {
            return Ok(reply);
        }
        Err(format!("{}: {line}: {}", self.who, reply["error"]))
    }
}

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

impl<S: Link, C: Link> Lockstep<S, C> {
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

fn value<T: std::str::FromStr>(args: &[String], name: &str, default: T) -> Result<T, String> {
    match crate::flag(args, name) {
        Some(text) => text.parse().map_err(|_| format!("bad {name}")),
        None => Ok(default),
    }
}

impl Plan {
    fn parse(args: &[String], level: Level) -> Result<Self, String> {
        let at = crate::flag(args, "--at")
            .map(|list| {
                list.split(',')
                    .map(|index| index.parse().map_err(|_| "bad --at".to_string()))
                    .collect::<Result<Vec<u64>, String>>()
            })
            .transpose()?
            .unwrap_or_default();
        let every = value(args, "--every", if at.is_empty() { 1 } else { 0 })?;
        let hold = crate::flag(args, "--hold")
            .map(|text| {
                let (tick, seconds) = text.split_once(':').ok_or("--hold is tick:seconds")?;
                Ok::<_, String>((
                    tick.parse().map_err(|_| "bad --hold tick")?,
                    seconds.parse().map_err(|_| "bad --hold seconds")?,
                ))
            })
            .transpose()?;
        let plan = Self {
            server: crate::flag(args, "--server")
                .ok_or("--server: the step socket of the server")?
                .into(),
            out: crate::flag(args, "--out")
                .ok_or("--out: where to write")?
                .into(),
            ticks: value(args, "--ticks", 120)?,
            frames: value(args, "--frames", 4)?,
            lock_at: value(args, "--lock-at", 600)?,
            min_chunks: value(args, "--min-chunks", MIN_CHUNKS)?,
            acts: args
                .windows(2)
                .filter(|pair| pair[0] == "--act")
                .map(|pair| pair[1].clone())
                .collect(),
            every,
            at,
            hold,
            stop_server: args.iter().any(|arg| arg == "--stop-server"),
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

fn play<S: Link, C: Link>(
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

fn settle<S: Link, C: Link>(lock: &mut Lockstep<S, C>) -> Result<(), String> {
    lock.client.ask("clock virtual")?;
    lock.calibrate(Duration::from_millis(50))?;
    for _ in 0..SETTLE_TICKS {
        lock.tick()?;
        lock.client.ask(&format!("step {TICK_MS}"))?;
    }
    Ok(())
}

fn wait_for_server(path: &Path) -> Result<Socket, String> {
    let start = Instant::now();
    loop {
        match Socket::open("server", path) {
            Ok(socket) => return Ok(socket),
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
        control::chore(&self.client.name, Chore::State, Duration::from_millis(500))
            .ok()
            .and_then(|line| serde_json::from_str::<Value>(&line).ok())
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
    server: &mut impl Link,
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

fn passage(server: &mut impl Link, client: &Client, plan: &Plan) -> Result<(), String> {
    let joiner = Joiner {
        client,
        min_chunks: plan.min_chunks,
    };
    let pace = Duration::from_millis(1000 / TICKS_PER_SECOND);
    join(server, &joiner, plan.lock_at, pace)?;
    let socket = Socket::over(
        "client",
        control::Probe::open(&client.name)?.into_stream(),
        ANSWER_LIMIT,
    )?;
    let mut lock = Lockstep {
        server,
        client: socket,
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
fn finish(server: &mut impl Link, stop: bool, result: Result<(), String>) -> Result<(), String> {
    if !stop {
        return result;
    }
    let stopped = server.ask("stop").map(|_| ());
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
mod tests {
    use std::collections::VecDeque;

    use serde_json::json;

    use super::*;

    #[derive(Default)]
    struct Script {
        heard: Vec<String>,
        answers: VecDeque<Result<Value, String>>,
    }

    impl Link for Script {
        fn ask(&mut self, line: &str) -> Result<Value, String> {
            self.heard.push(line.to_string());
            self.answers.pop_front().expect(line)
        }
    }

    fn script(answers: Vec<Result<Value, String>>) -> Script {
        Script {
            heard: Vec::new(),
            answers: answers.into(),
        }
    }

    fn plan(args: &[&str]) -> Result<Plan, String> {
        let mut all = vec!["--server", "s", "--out", "o"];
        all.extend(args);
        let all: Vec<String> = all.iter().map(|arg| arg.to_string()).collect();
        Plan::parse(&all, Level::Render)
    }

    fn sent(client_in: u64, out: u64) -> Result<Value, String> {
        Ok(json!({"in": client_in, "out": out, "active": true}))
    }

    #[test]
    fn a_tick_passes_what_each_side_owes_the_other() {
        let mut lock = Lockstep {
            server: script(vec![Ok(json!({"tick": 7, "sent": 110, "received": 53}))]),
            client: script(vec![sent(40, 13), Ok(json!({"in": 50}))]),
            origin: Origin {
                sent: 100,
                received: 50,
                client_in: 40,
                client_out: 10,
            },
        };
        assert_eq!(lock.tick(), Ok(7));
        assert_eq!(lock.server.heard, ["step 1 53"]);
        assert_eq!(lock.client.heard, ["sent", "fence 50"]);
    }

    #[test]
    fn a_client_cut_from_the_server_stops_the_passage() {
        let mut lock = Lockstep {
            server: script(vec![]),
            client: script(vec![Ok(json!({"in": 1, "out": 1, "active": false}))]),
            origin: Origin::default(),
        };
        assert!(lock.tick().unwrap_err().contains("lost its connection"));
        assert!(lock.server.heard.is_empty());
    }

    #[test]
    fn an_expired_barrier_is_an_error_and_nothing_goes_on() {
        let mut lock = Lockstep {
            server: script(vec![Err("server: fence expired: received 1 of 2".into())]),
            client: script(vec![sent(0, 2)]),
            origin: Origin::default(),
        };
        assert!(lock.tick().unwrap_err().contains("fence expired"));
        assert_eq!(lock.client.heard, ["sent"]);
    }

    #[test]
    fn calibration_waits_for_counters_that_stopped_moving() {
        let status = |sent: u64| Ok(json!({"sent": sent, "received": 5}));
        let mut server = vec![status(9)];
        server.extend((0..QUIET_POLLS + 1).map(|_| status(10)));
        let mut client = vec![sent(8, 3)];
        client.extend((0..QUIET_POLLS + 1).map(|_| sent(10, 3)));
        let mut lock = Lockstep {
            server: script(server),
            client: script(client),
            origin: Origin::default(),
        };
        lock.calibrate(Duration::ZERO).unwrap();
        assert_eq!(
            lock.origin,
            Origin {
                sent: 10,
                received: 5,
                client_in: 10,
                client_out: 3
            }
        );
    }

    #[test]
    fn a_passage_writes_one_measure_per_frame_and_keeps_the_wanted_pictures() {
        let camera = || Ok(json!({"x": 1.5, "y": 2.0, "z": -3.25, "yaw": 90.0, "pitch": 0.5}));
        let mut client = vec![sent(0, 0), Ok(json!({"in": 0})), camera()];
        client.push(Ok(json!({"path": "/shots/f00000.png"})));
        client.push(camera());
        client.push(sent(0, 0));
        let mut lock = Lockstep {
            server: script(vec![Ok(json!({"tick": 1, "sent": 0, "received": 0}))]),
            client: script(client),
            origin: Origin::default(),
        };
        let plan = plan(&["--ticks", "1", "--frames", "2", "--every", "2"]).unwrap();
        let mut measures = Vec::new();
        let mut kept = Vec::new();
        let captured = play(&mut lock, &plan, &mut measures, |name, path| {
            kept.push((name.to_string(), path.to_path_buf()));
            Ok(())
        })
        .unwrap();
        assert_eq!(captured, 1);
        assert_eq!(kept[0].0, "f00000");
        assert!(lock.client.heard.contains(&"step 25".to_string()));
        let lines = String::from_utf8(measures).unwrap();
        assert_eq!(
            lines.lines().nth(1),
            Some(r#"{"t":0,"k":1,"x":1.5,"y":2.0,"z":-3.25,"yaw":90.0,"pitch":0.5}"#)
        );
        assert_eq!(lines.lines().count(), 2);
    }

    #[test]
    fn a_client_cut_on_the_last_tick_fails_the_passage() {
        let client = vec![
            sent(0, 0),
            Ok(json!({"in": 0})),
            Ok(json!({"x": 0, "y": 0, "z": 0, "yaw": 0, "pitch": 0})),
            Ok(json!({"in": 0, "out": 0, "active": false})),
        ];
        let mut lock = Lockstep {
            server: script(vec![Ok(json!({"tick": 1, "sent": 0, "received": 0}))]),
            client: script(client),
            origin: Origin::default(),
        };
        let plan = plan(&["--ticks", "1", "--frames", "1", "--every", "0"]).unwrap();
        let error = play(&mut lock, &plan, &mut Vec::new(), |_, _| Ok(())).unwrap_err();
        assert!(error.contains("lost its connection"));
    }

    struct Fake {
        alive: bool,
        in_game: std::cell::RefCell<VecDeque<bool>>,
    }

    impl Joining for Fake {
        fn alive(&self) -> bool {
            self.alive
        }

        fn in_game(&self) -> bool {
            self.in_game.borrow_mut().pop_front().unwrap_or(false)
        }

        fn name(&self) -> &str {
            "fake"
        }

        fn min_chunks(&self) -> u64 {
            200
        }
    }

    fn fake(in_game: &[bool]) -> Fake {
        Fake {
            alive: true,
            in_game: std::cell::RefCell::new(in_game.iter().copied().collect()),
        }
    }

    fn ticks(from: u64, to: u64) -> Script {
        let mut answers: Vec<_> = vec![Ok(json!({"tick": from}))];
        answers.extend((from + 1..=to).map(|tick| Ok(json!({"tick": tick}))));
        script(answers)
    }

    #[test]
    fn a_server_that_does_not_count_ends_the_wait() {
        let mut server = script(vec![Ok(json!({"tick": 0})), Ok(json!({"tick": 0}))]);
        let error = join(&mut server, &fake(&[]), 600, Duration::ZERO).unwrap_err();
        assert!(error.contains("does not count"));
    }

    #[test]
    fn a_client_gone_from_the_game_at_the_lock_is_not_in_game() {
        let error = join(&mut ticks(0, 10), &fake(&[true, false]), 10, Duration::ZERO).unwrap_err();
        assert!(error.contains("not in game with 200 chunks at tick 10"));
        assert!(error.contains("--min-chunks"));
    }

    #[test]
    fn one_slow_answer_at_the_lock_is_not_a_failure() {
        let client = fake(&[true, false, false, true]);
        assert_eq!(join(&mut ticks(0, 10), &client, 10, Duration::ZERO), Ok(()));
    }

    #[test]
    fn the_lock_comes_with_the_client_in_game() {
        let client = fake(&[true, true]);
        assert_eq!(join(&mut ticks(0, 10), &client, 10, Duration::ZERO), Ok(()));
    }

    #[test]
    fn a_server_past_the_lock_is_refused() {
        let mut server = script(vec![Ok(json!({"tick": 700}))]);
        let error = join(&mut server, &fake(&[]), 600, Duration::ZERO).unwrap_err();
        assert!(error.contains("restart it"));
    }

    #[test]
    fn a_failed_passage_still_stops_the_server() {
        let mut server = script(vec![Ok(json!({"ok": true}))]);
        let result = finish(&mut server, true, Err("the client is gone".into()));
        assert_eq!(result, Err("the client is gone".to_string()));
        assert_eq!(server.heard, ["stop"]);
    }

    #[test]
    fn the_server_stays_unless_asked() {
        let mut server = script(vec![]);
        assert_eq!(finish(&mut server, false, Ok(())), Ok(()));
        assert!(server.heard.is_empty());
    }

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("voidc-step-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_run_starts_without_what_the_last_one_wrote() {
        let dir = folder("prepare");
        for name in [
            "f00000.png",
            "f00479.png",
            "video.mp4",
            "sheet.png",
            "frames.jsonl",
        ] {
            fs::write(dir.join(name), b"old").unwrap();
        }
        fs::write(dir.join("notes.txt"), b"mine").unwrap();
        fs::write(dir.join("fond.png"), b"mine").unwrap();
        prepare(&dir).unwrap();
        let mut left: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, ["fond.png", "notes.txt"]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn outputs_are_what_the_glob_of_the_video_takes() {
        assert!(is_output("f00012.png") && is_output("f1x.png"));
        assert!(!is_output("foo.png") && !is_output("f.png"));
    }

    #[test]
    fn a_silent_peer_is_named_in_the_error() {
        let (ours, _theirs) = UnixStream::pair().unwrap();
        let mut socket = Socket::over("client", ours, Duration::from_millis(20)).unwrap();
        let error = socket.ask("sent").unwrap_err();
        assert!(error.contains("client gave no answer to sent"), "{error}");
    }

    #[test]
    fn a_folder_with_a_bracket_still_makes_a_video_and_a_sheet() {
        let dir = folder("a[b]");
        let made = Command::new("ffmpeg")
            .current_dir(&dir)
            .args(["-y", "-loglevel", "error", "-f", "lavfi", "-i"])
            .args(["color=red:size=64x36:rate=1:duration=3", "f%05d.png"])
            .status();
        if !made.is_ok_and(|status| status.success()) {
            eprintln!("no ffmpeg: skipped");
            return;
        }
        fs::write(dir.join("foo.png"), b"not a picture").unwrap();
        assemble(&dir, 4, 3).unwrap();
        assert!(dir.join("video.mp4").exists() && dir.join("sheet.png").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn hand_picked_pictures_play_slowly_and_a_stride_plays_at_game_speed() {
        assert_eq!(
            plan(&["--at", "3,7"]).unwrap().video_rate(),
            SPARSE_VIDEO_RATE
        );
        assert_eq!(plan(&["--every", "1"]).unwrap().video_rate(), 80);
        assert_eq!(plan(&["--every", "4"]).unwrap().video_rate(), 20);
    }

    #[test]
    fn the_chunks_to_wait_for_are_an_option() {
        assert_eq!(plan(&[]).unwrap().min_chunks, MIN_CHUNKS);
        assert_eq!(plan(&["--min-chunks", "50"]).unwrap().min_chunks, 50);
    }

    #[test]
    fn pictures_are_wanted_by_stride_or_by_index() {
        let every = plan(&["--every", "4"]).unwrap();
        assert!(every.wanted(0) && every.wanted(8) && !every.wanted(3));
        let some = plan(&["--at", "3,7"]).unwrap();
        assert!(some.wanted(7) && !some.wanted(0));
        let none = plan(&["--every", "0"]).unwrap();
        assert!(!none.captures());
    }

    #[test]
    fn the_logic_level_cannot_capture() {
        let args: Vec<String> = ["--server", "s", "--out", "o"]
            .iter()
            .map(|arg| arg.to_string())
            .collect();
        assert!(Plan::parse(&args, Level::Logic).is_err());
        let mut none = args.clone();
        none.extend(["--every".to_string(), "0".to_string()]);
        assert!(Plan::parse(&none, Level::Logic).is_ok());
    }

    #[test]
    fn a_plan_needs_a_server_a_folder_and_a_frame() {
        assert!(Plan::parse(&[], Level::Render).is_err());
        assert!(plan(&["--frames", "0"]).is_err());
        assert_eq!(plan(&["--hold", "30:45"]).unwrap().hold, Some((30, 45)));
        assert!(plan(&["--hold", "30"]).is_err());
    }

    #[test]
    fn a_sheet_picks_evenly_and_never_more_than_it_holds() {
        assert!(sheet_filter(480).ends_with("tile=6x4"));
        assert!(sheet_filter(480).contains("mod(n\\,20)"));
        assert!(sheet_filter(5).ends_with("tile=5x1"));
        assert!(sheet_filter(25).contains("mod(n\\,2)"));
        assert!(sheet_filter(25).ends_with("tile=6x3"));
    }

    #[test]
    fn every_act_is_kept_in_order() {
        let acts = plan(&["--act", "tp 80 90 0", "--act", "tell @r hi"])
            .unwrap()
            .acts;
        assert_eq!(acts, ["tp 80 90 0", "tell @r hi"]);
        assert!(plan(&[]).unwrap().acts.is_empty());
    }

    #[test]
    fn step_refuses_a_probe_of_another_version() {
        let stale = control::stale::Stale::start(Level::Render);
        let client = registry::load(control::stale::NAME).unwrap();
        let out = registry::home().join("out");
        let plan = plan(&["--lock-at", "10", "--min-chunks", "1"])
            .map(|plan| Plan { out, ..plan })
            .unwrap();
        let mut ticks = vec![Ok(json!({"tick": 0}))];
        ticks.extend((1..=10).map(|tick| Ok(json!({"tick": tick}))));
        stale.assert_refused(passage(&mut script(ticks), &client, &plan));
    }
}
