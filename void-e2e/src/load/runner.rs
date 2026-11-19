use std::f64::consts::TAU;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use azalea_core::position::{BlockPos, ChunkPos, Vec3};
use azalea_protocol::packets::PROTOCOL_VERSION;
use azalea_protocol::packets::game::ClientboundGamePacket;
use tokio::time::{Instant, MissedTickBehavior};
use voidmc::{GameTime, ServerConfigResource, TIME_SYNC_INTERVAL, WorldTime};

use super::measure::{
    CheckOutcome, Connections, Distribution, ErrorKind, Errors, Latency, Measurements, Resources,
    SmokeCheck, TickRate,
};
use super::report::{Conditions, Report, ServerSettings};
use super::system;
use super::workload::Workload;
use crate::bot::{Bot, BotOptions};
use crate::logs::LogRecord;
use crate::server::{TestServer, TestServerBuilder};

/// Where the load clients connect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A server the tool starts in this process on an ephemeral port.
    InProcess,
    /// An address the operator supplied, such as a private staging server.
    Endpoint(String),
}

const SMOKE_CHECKS: [&str; 7] = [
    "status_ping",
    "join",
    "chunks_loaded",
    "chat_echo",
    "command_reply",
    "chunk_boundary_crossing",
    "healthy",
];

/// Runs the smoke checks, then the load, against `target`, and judges the
/// result. Fails only when an operator endpoint cannot be resolved.
pub async fn run(workload: &Workload, target: Target) -> Result<Report, String> {
    match target {
        Target::InProcess => Ok(run_in_process(workload, TestServer::builder()).await),
        Target::Endpoint(endpoint) => run_endpoint(workload, &endpoint).await,
    }
}

/// Runs `workload` against a server started from `builder`, at the
/// workload's nominal tick rate. Server-side tick rate, resource use and
/// logs are measured too.
pub async fn run_in_process(workload: &Workload, builder: TestServerBuilder) -> Report {
    let started = Instant::now();
    let tick_rate = workload.nominal_tick_rate;
    let server = builder
        .config(|config| config.tick_rate(tick_rate))
        .start()
        .await;
    let settings = server
        .with_world(|world| {
            // A running overworld clock, as every vanilla world has: its
            // world-time updates let the clients measure the tick rate too.
            world.spawn(WorldTime::new());
            let config = world.resource::<ServerConfigResource>();
            ServerSettings {
                tick_rate: config.tick_rate,
                view_distance: config.view_distance,
                simulation_distance: config.simulation_distance,
                max_players: config.max_players,
            }
        })
        .await;
    let address = server.address();
    let mut conditions = Conditions::capture("in-process", address.to_string(), Some(settings));
    tracing::info!(workload = %workload.id(), %address, "load run against an in-process server");

    let mut unavailable = Vec::new();
    let mut measurements = phases(workload, address, Some(&server), &mut unavailable).await;

    let failures: Vec<LogRecord> = server
        .logs()
        .into_iter()
        .filter(LogRecord::is_failure)
        .collect();
    measurements.errors.server_log_failures = Some(failures.len() as u64);
    for failure in &failures {
        measurements
            .errors
            .detail("server log", &failure.to_string());
    }
    let _ = tokio::task::spawn_blocking(move || drop(server)).await;

    conditions.wall_clock_ms = started.elapsed().as_millis() as u64;
    Report::new(workload, conditions, measurements, unavailable)
}

/// Runs `workload` against an operator-supplied `host:port`. The server is
/// outside this process: its own tick count, resource use and logs are
/// reported as unavailable.
pub async fn run_endpoint(workload: &Workload, endpoint: &str) -> Result<Report, String> {
    let started = Instant::now();
    let address = tokio::net::lookup_host(endpoint)
        .await
        .map_err(|error| format!("cannot resolve the endpoint {endpoint:?}: {error}"))?
        .next()
        .ok_or_else(|| format!("the endpoint {endpoint:?} resolved to no address"))?;
    let mut conditions = Conditions::capture("operator-endpoint", endpoint.to_string(), None);
    tracing::info!(workload = %workload.id(), %address, "load run against an operator endpoint");

    let mut unavailable = vec![
        "tick_rate.server_observed: the server is not in this process; min_tick_rate is judged \
         on the world-time updates the clients receive"
            .to_string(),
        "resources: the CPU and memory of a server outside this process cannot be observed by \
         the load clients; measure them on the server host"
            .to_string(),
        "errors.server_log_failures: the server log is not visible from the load clients; check \
         it on the server host"
            .to_string(),
    ];
    let measurements = phases(workload, address, None, &mut unavailable).await;
    conditions.wall_clock_ms = started.elapsed().as_millis() as u64;
    Ok(Report::new(workload, conditions, measurements, unavailable))
}

async fn phases(
    workload: &Workload,
    address: SocketAddr,
    server: Option<&TestServer>,
    unavailable: &mut Vec<String>,
) -> Measurements {
    let run = run_id();
    let smoke_checks = smoke(address, workload, format!("smoke-{run:04x}")).await;
    let smoke_passed = smoke_checks
        .iter()
        .all(|check| check.outcome == CheckOutcome::Passed);
    let mut measurements = Measurements {
        smoke_checks,
        tick_rate: TickRate {
            nominal: workload.nominal_tick_rate as f64,
            ..Default::default()
        },
        ..Default::default()
    };
    if smoke_passed {
        load(
            workload,
            address,
            run,
            server,
            &mut measurements,
            unavailable,
        )
        .await;
    } else {
        unavailable.push("load phase: skipped because a smoke check failed".to_string());
    }
    measurements
}

fn run_id() -> u16 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos());
    (nanos ^ std::process::id()) as u16
}

/// Runs `future` in its own task and turns a panic, which is how the harness
/// reports a failure, into its message.
async fn attempt<T: Send + 'static>(
    future: impl Future<Output = T> + Send + 'static,
) -> Result<T, String> {
    match tokio::spawn(future).await {
        Ok(value) => Ok(value),
        Err(error) if error.is_panic() => {
            let payload = error.into_panic();
            Err(payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "panicked".to_string()))
        }
        Err(error) => Err(error.to_string()),
    }
}

#[derive(Default)]
struct Checks(Vec<SmokeCheck>);

impl Checks {
    /// Runs one check; `None` when it failed, after recording why.
    async fn run<T: Send + 'static>(
        &mut self,
        name: &'static str,
        check: impl Future<Output = (T, String)> + Send + 'static,
    ) -> Option<T> {
        let (outcome, detail, value) = match attempt(check).await {
            Ok((value, detail)) => (CheckOutcome::Passed, detail, Some(value)),
            Err(message) => (CheckOutcome::Failed, message, None),
        };
        tracing::info!(check = name, ?outcome, %detail, "smoke check");
        self.0.push(SmokeCheck {
            name,
            outcome,
            detail,
        });
        value
    }

    /// The checks run so far, then every other check as skipped.
    fn finish(mut self) -> Vec<SmokeCheck> {
        for name in SMOKE_CHECKS {
            if !self.0.iter().any(|check| check.name == name) {
                self.0.push(SmokeCheck {
                    name,
                    outcome: CheckOutcome::Skipped,
                    detail: "an earlier check failed".to_string(),
                });
            }
        }
        self.0
    }
}

/// One client walks through the functional path a player takes: server
/// list, join, chunks, chat, a command, a chunk-boundary crossing, and ends
/// with no protocol anomaly.
async fn smoke(address: SocketAddr, workload: &Workload, name: String) -> Vec<SmokeCheck> {
    let mut checks = Checks::default();
    let status = checks
        .run("status_ping", async move {
            let status = crate::timeout("the status response", crate::bot::status(address)).await;
            assert_eq!(
                status.version.protocol, PROTOCOL_VERSION,
                "the server speaks another protocol version"
            );
            (
                (),
                format!(
                    "protocol {}, {}/{} players online",
                    status.version.protocol, status.players.online, status.players.max
                ),
            )
        })
        .await;
    if status.is_none() {
        return checks.finish();
    }

    let Some(bot) = checks
        .run("join", async move {
            let bot = Bot::join(address, &name, BotOptions::default()).await;
            let detail = format!(
                "{} logged in, configured with {} registries, spawned as entity {}",
                bot.name(),
                bot.transcript().registries.len(),
                bot.entity_id()
            );
            (Arc::new(bot), detail)
        })
        .await
    else {
        return checks.finish();
    };

    let b = bot.clone();
    let chunks = checks
        .run("chunks_loaded", async move {
            b.wait_until("the chunk under its feet", |view| {
                view.ground_below(BlockPos::from(view.position)).is_some()
            })
            .await;
            (
                (),
                format!("{} chunks held", b.view(|view| view.chunks.len())),
            )
        })
        .await;
    if chunks.is_none() {
        return checks.finish();
    }

    let b = bot.clone();
    let chat = checks
        .run("chat_echo", async move {
            let token = format!("[{}#smoke]", b.name());
            let ms = chat_round_trip(&b, &token).await;
            ((), format!("chat broadcast back in {ms:.1} ms"))
        })
        .await;
    if chat.is_none() {
        return checks.finish();
    }

    let b = bot.clone();
    let (command, reply) = (workload.command, workload.command_reply);
    let replied = checks
        .run("command_reply", async move {
            let ms = command_round_trip(&b, command, reply).await;
            ((), format!("/{command} answered in {ms:.1} ms"))
        })
        .await;
    if replied.is_none() {
        return checks.finish();
    }

    let b = bot.clone();
    let crossed = checks
        .run("chunk_boundary_crossing", async move {
            let origin = b.position();
            let mut position = origin;
            for _ in 0..4 {
                position = Vec3::new(position.x + 4.0, position.y, position.z);
                b.move_to(position).await;
            }
            let target = ChunkPos::from(BlockPos::from(position));
            b.wait_until("the chunk view re-centred one chunk east", |view| {
                view.chunk_center == Some(target)
            })
            .await;
            (
                (),
                format!("view re-centred on chunk {} {}", target.x, target.z),
            )
        })
        .await;
    if crossed.is_none() {
        return checks.finish();
    }

    let b = bot.clone();
    checks
        .run("healthy", async move {
            let anomalies = b.anomalies();
            assert!(anomalies.is_empty(), "protocol anomalies: {anomalies:?}");
            ((), "no protocol anomaly".to_string())
        })
        .await;
    if let Ok(bot) = Arc::try_unwrap(bot) {
        bot.disconnect().await;
    }
    checks.finish()
}

#[derive(Default)]
struct Collector {
    connected: u64,
    chat: Vec<f64>,
    command: Vec<f64>,
    client_tick_rates: Vec<f64>,
    errors: Errors,
}

type Sink = Arc<Mutex<Collector>>;

async fn load(
    workload: &Workload,
    address: SocketAddr,
    run: u16,
    server: Option<&TestServer>,
    measurements: &mut Measurements,
    unavailable: &mut Vec<String>,
) {
    measurements.load_ran = true;
    let sink = Sink::default();
    let start = Instant::now();
    let ramp_up = Duration::from_millis(workload.ramp_up_ms);
    let steady_start = start + ramp_up;
    let deadline = steady_start + Duration::from_millis(workload.steady_ms);

    let clients: Vec<_> = (0..workload.clients)
        .map(|index| {
            let join_at = start + ramp_up.mul_f64(index as f64 / workload.clients as f64);
            tokio::spawn(load_client(
                index,
                address,
                workload.clone(),
                format!("load-{run:04x}-{index}"),
                join_at,
                deadline,
                sink.clone(),
            ))
        })
        .collect();
    let all_done = async {
        for client in clients {
            if let Err(error) = client.await {
                let message = error.to_string();
                sink.lock()
                    .unwrap()
                    .errors
                    .record(ErrorKind::Other, "load client", &message);
            }
        }
    };
    let observe = async {
        match server {
            Some(server) => Some(monitor(server, steady_start, deadline).await),
            None => None,
        }
    };
    let ((), window) = tokio::join!(all_done, observe);

    let collector = std::mem::take(&mut *sink.lock().unwrap());
    measurements.connections = Connections::new(workload.clients as u64, collector.connected);
    let all: Vec<f64> = collector
        .chat
        .iter()
        .chain(&collector.command)
        .copied()
        .collect();
    measurements.latency = Latency {
        all: Distribution::of(&all),
        chat: Distribution::of(&collector.chat),
        command: Distribution::of(&collector.command),
    };
    let mut rates = collector.client_tick_rates;
    if rates.is_empty() {
        unavailable.push(
            "tick_rate.client_observed: no client received two world-time updates; the server \
             may run without a world clock"
                .to_string(),
        );
    }
    rates.sort_by(f64::total_cmp);
    measurements.tick_rate.client_samples = rates.len() as u64;
    measurements.tick_rate.client_observed_min = rates.first().copied();
    measurements.tick_rate.client_observed_median = rates.get(rates.len() / 2).copied();
    measurements.errors = collector.errors;

    if let Some(window) = window {
        measurements.tick_rate.server_observed = Some(window.tick_rate);
        if window.resources.game_thread_cpu_ms.is_none() {
            unavailable.push(
                "resources.game_thread_cpu_ms: no per-thread CPU accounting on this OS".to_string(),
            );
        }
        if window.resources.process_cpu_ms.is_none() {
            unavailable.push(
                "resources.process_cpu_ms and process_rss_peak_kib: no procfs on this OS"
                    .to_string(),
            );
        }
        measurements.resources = Some(window.resources);
    }
}

async fn load_client(
    index: usize,
    address: SocketAddr,
    workload: Workload,
    name: String,
    join_at: Instant,
    deadline: Instant,
    sink: Sink,
) {
    tokio::time::sleep_until(join_at).await;
    let join_name = name.clone();
    let joined =
        attempt(async move { Bot::join(address, &join_name, BotOptions::default()).await }).await;
    let bot = match joined {
        Ok(bot) => Arc::new(bot),
        Err(message) => {
            sink.lock()
                .unwrap()
                .errors
                .connection_failure(&name, &message);
            return;
        }
    };
    sink.lock().unwrap().connected += 1;

    let driven = attempt(drive(bot.clone(), index, workload, deadline, sink.clone())).await;
    {
        let mut collector = sink.lock().unwrap();
        if let Err(message) = driven {
            collector
                .errors
                .record(ErrorKind::classify(&message), &name, &message);
        }
        for anomaly in bot.anomalies() {
            collector.errors.record(
                ErrorKind::Protocol,
                &name,
                &format!("protocol anomaly: {anomaly}"),
            );
        }
    }
    if let Ok(bot) = Arc::try_unwrap(bot) {
        bot.disconnect().await;
    }
}

/// One load client's action cycles until `deadline`.
async fn drive(bot: Arc<Bot>, index: usize, workload: Workload, deadline: Instant, sink: Sink) {
    let origin = bot.position();
    let heading = TAU * index as f64 / workload.clients as f64;
    let patience = world_time_patience(&workload);
    let first = world_time(&bot, patience).await;

    let mut ticker = tokio::time::interval(Duration::from_millis(workload.action_interval_ms));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut cycle = 0u64;
    while Instant::now() < deadline {
        ticker.tick().await;
        cycle += 1;
        let distance = walked(cycle, workload.step_blocks, workload.roam_radius_blocks);
        bot.move_to(Vec3::new(
            origin.x + heading.cos() * distance,
            origin.y,
            origin.z + heading.sin() * distance,
        ))
        .await;
        let chat = chat_round_trip(&bot, &format!("[{}#{cycle}]", bot.name())).await;
        let command = command_round_trip(&bot, workload.command, workload.command_reply).await;
        let mut collector = sink.lock().unwrap();
        collector.chat.push(chat);
        collector.command.push(command);
    }

    let Some(first) = first else {
        return;
    };
    if let Some(last) = world_time(&bot, patience).await {
        let seconds = (last.0 - first.0).as_secs_f64();
        if seconds > 0.0 && last.1 > first.1 {
            let rate = (last.1 - first.1) as f64 / seconds;
            sink.lock().unwrap().client_tick_rates.push(rate);
        }
    }
}

/// How long a client waits for a world-time update: two sync intervals at
/// the nominal tick rate. A server without a running world clock sends none,
/// and the clients then do not measure the tick rate.
fn world_time_patience(workload: &Workload) -> Duration {
    let interval = f64::from(TIME_SYNC_INTERVAL) / workload.nominal_tick_rate.max(1) as f64;
    Duration::from_secs_f64(2.0 * interval)
}

/// Distance from spawn after `cycle` steps: out to `radius`, then back, and
/// again.
fn walked(cycle: u64, step: f64, radius: f64) -> f64 {
    let span = 2.0 * radius;
    let travelled = (cycle as f64 * step) % span;
    if travelled <= radius {
        travelled
    } else {
        span - travelled
    }
}

/// Waits up to `patience` for the next world-time update and returns when it
/// arrived and the game time it carries.
async fn world_time(bot: &Bot, patience: Duration) -> Option<(Instant, u64)> {
    bot.clear();
    let update = bot.expect("a world time update", |packet| match packet {
        ClientboundGamePacket::SetTime(time) => Some(time.game_time),
        _ => None,
    });
    let game_time = tokio::time::timeout(patience, update).await.ok()?;
    Some((Instant::now(), game_time))
}

fn system_chat_contains(packet: &ClientboundGamePacket, text: &str) -> Option<()> {
    match packet {
        ClientboundGamePacket::SystemChat(chat) => {
            chat.content.to_string().contains(text).then_some(())
        }
        _ => None,
    }
}

/// Milliseconds from sending `token` as chat to receiving its broadcast.
async fn chat_round_trip(bot: &Bot, token: &str) -> f64 {
    bot.clear();
    let sent = Instant::now();
    bot.chat(token).await;
    bot.expect("its chat message back", |packet| {
        system_chat_contains(packet, token)
    })
    .await;
    sent.elapsed().as_secs_f64() * 1e3
}

/// Milliseconds from running `/command` to receiving a reply containing
/// `reply`.
async fn command_round_trip(bot: &Bot, command: &str, reply: &str) -> f64 {
    bot.clear();
    let sent = Instant::now();
    bot.command(command).await;
    bot.expect("the command reply", |packet| {
        system_chat_contains(packet, reply)
    })
    .await;
    sent.elapsed().as_secs_f64() * 1e3
}

struct Window {
    tick_rate: f64,
    resources: Resources,
}

struct Sample {
    at: Instant,
    game_time: i64,
    /// The game thread's and the server threads' CPU time, in nanoseconds.
    cpu_ns: Option<(u64, u64)>,
    process_cpu_ms: Option<f64>,
}

async fn sample(server: &TestServer) -> Sample {
    // Both CPU readings come from one pass on the game thread, in integer
    // nanoseconds, so the server threads' total holds exactly the game
    // thread's time read here. Read apart, or summed in floating point, the
    // total could grow less than the game thread's own time over the window
    // when the game thread does nearly all of the server's work (the
    // single-threaded executor).
    let (game_time, cpu_ns) = server
        .with_world(|world| {
            (
                world.resource::<GameTime>().0,
                system::thread_and_threads_cpu_ns(&system::SERVER_THREADS),
            )
        })
        .await;
    Sample {
        at: Instant::now(),
        game_time,
        cpu_ns,
        process_cpu_ms: system::process_cpu_ms(),
    }
}

/// Measures the in-process server over the steady window: ticks from its
/// game time, CPU of its game thread, and the process's CPU and memory.
async fn monitor(server: &TestServer, from: Instant, until: Instant) -> Window {
    tokio::time::sleep_until(from).await;
    let first = sample(server).await;
    let mut rss_peak = system::process_rss_kib();
    loop {
        let now = Instant::now();
        if now >= until {
            break;
        }
        tokio::time::sleep((until - now).min(Duration::from_millis(250))).await;
        rss_peak = rss_peak.max(system::process_rss_kib());
    }
    let last = sample(server).await;

    let window_ms = (last.at - first.at).as_secs_f64() * 1e3;
    let delta = |a: Option<f64>, b: Option<f64>| Some(b? - a?);
    let cpu_ms = |pick: fn((u64, u64)) -> u64| {
        Some(system::ms_between(pick(first.cpu_ns?), pick(last.cpu_ns?)))
    };
    let game_thread_cpu_ms = cpu_ms(|(game, _)| game);
    let server_threads_cpu_ms = cpu_ms(|(_, server)| server);
    Window {
        tick_rate: (last.game_time - first.game_time) as f64 / (window_ms / 1e3),
        resources: Resources {
            window_ms,
            game_thread_cpu_ms,
            game_thread_utilisation: game_thread_cpu_ms.map(|cpu| cpu / window_ms),
            server_threads_cpu_ms,
            server_threads_utilisation: server_threads_cpu_ms.map(|cpu| cpu / window_ms),
            process_cpu_ms: delta(first.process_cpu_ms, last.process_cpu_ms),
            process_rss_peak_kib: rss_peak,
            scope: "game thread: the thread running the server's schedule; server threads: \
                    the game thread, Bevy task pools and the network runtime; process: the \
                    server and the load clients, which share this process",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clients_walk_out_to_the_radius_and_back() {
        let distances: Vec<f64> = (0..=10).map(|cycle| walked(cycle, 4.0, 16.0)).collect();
        assert_eq!(
            distances,
            [0.0, 4.0, 8.0, 12.0, 16.0, 12.0, 8.0, 4.0, 0.0, 4.0, 8.0]
        );
    }
}
