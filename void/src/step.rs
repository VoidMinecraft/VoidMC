//! Runs the server one tick at a time, on the orders of a unix socket.
//!
//! Each line is an order and gets one JSON line back:
//!
//! - `step <count> [received]` waits until `received` packets have arrived, then runs `count` ticks.
//! - `status` reports the counters without running anything.
//! - `stop` ends the server through `AppExit`, after one last tick that lets systems see it.
//!
//! The counters make the barrier between the server and one client: a pilot reads what the client
//! wrote and passes it as `received`, then waits for the client to have read `sent`.
//!
//! The runner blocks on the socket, so nothing would run `TerminalCtrlCHandlerPlugin`: this mode
//! leaves it out, and Ctrl-C ends the process at once, without `AppExit`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

use bevy_app::{App, AppExit, Plugin, PluginsState};
use bevy_ecs::world::World;
use flume::Sender;

use crate::network::{ClientSenders, IncomingPacket};

const FENCE_LIMIT: Duration = Duration::from_secs(10);
const FENCE_POLL: Duration = Duration::from_micros(50);

/// The packets of the barrier: `sent` counted where they are queued for a client, `received`
/// once the game can read them.
#[derive(Default)]
pub(crate) struct Counters {
    sent: Arc<AtomicU64>,
    received: AtomicU64,
}

/// Puts a counter in front of the game's channel.
pub(crate) fn counted(
    game: Sender<IncomingPacket>,
    counters: Arc<Counters>,
) -> Sender<IncomingPacket> {
    relay(game, counters)
}

fn relay<T: Send + 'static>(game: Sender<T>, counters: Arc<Counters>) -> Sender<T> {
    let (network, packets) = flume::unbounded::<T>();
    std::thread::spawn(move || {
        for packet in packets.iter() {
            if game.send(packet).is_err() {
                return;
            }
            counters.received.fetch_add(1, Ordering::SeqCst);
        }
    });
    network
}

/// Replaces the tick loop: the server stands still until a pilot asks for a tick.
pub(crate) struct StepPlugin {
    socket: PathBuf,
    tick: Duration,
    counters: Arc<Counters>,
}

impl StepPlugin {
    pub(crate) fn new(socket: PathBuf, tick: Duration, counters: Arc<Counters>) -> Self {
        Self {
            socket,
            tick,
            counters,
        }
    }
}

impl Plugin for StepPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(crate::pace::SteppedPacePlugin::new(self.tick));
        let socket = self.socket.clone();
        let counters = self.counters.clone();
        app.set_runner(move |app| run(app, &socket, &counters));
    }

    fn finish(&self, app: &mut App) {
        if let Some(mut senders) = app.world_mut().get_resource_mut::<ClientSenders>() {
            senders.count_sent(self.counters.sent.clone());
        }
    }
}

fn run(mut app: App, socket: &Path, counters: &Counters) -> AppExit {
    if app.plugins_state() != PluginsState::Cleaned {
        while app.plugins_state() == PluginsState::Adding {
            sleep(Duration::from_millis(1));
        }
        app.finish();
        app.cleanup();
    }
    if UnixStream::connect(socket).is_ok() {
        tracing::error!(socket = %socket.display(), "Another server answers on the step socket");
        return AppExit::error();
    }
    let _ = std::fs::remove_file(socket);
    let listener = match UnixListener::bind(socket) {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(socket = %socket.display(), %error, "Step socket refused");
            return AppExit::error();
        }
    };
    tracing::info!(socket = %socket.display(), "Server locked, waiting for steps");
    let exit = loop {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Some(exit) = serve(stream, &mut app, counters) {
                    break exit;
                }
            }
            Err(error) => {
                tracing::error!(%error, "Step socket failed");
                break AppExit::error();
            }
        }
    };
    let _ = std::fs::remove_file(socket);
    exit
}

fn serve(stream: UnixStream, app: &mut App, counters: &Counters) -> Option<AppExit> {
    let mut replies = stream.try_clone().ok()?;
    for line in BufReader::new(stream).lines() {
        let line = line.ok()?;
        let (reply, exit) = match Order::parse(&line) {
            Ok(order) => order.carry_out(app, counters),
            Err(error) => (failure(&error, app.world(), counters), None),
        };
        if writeln!(replies, "{reply}").is_err() || exit.is_some() {
            return exit;
        }
    }
    None
}

#[derive(Debug, PartialEq)]
enum Order {
    Step { ticks: u64, received: u64 },
    Status,
    Stop,
}

impl Order {
    fn parse(line: &str) -> Result<Self, String> {
        let mut words = line.split_whitespace();
        let number = |word: Option<&str>| {
            word.map_or(Ok(0), |word| {
                word.parse().map_err(|_| format!("bad number {word}"))
            })
        };
        match words.next() {
            Some("step") => {
                let ticks = words.next().ok_or("step: how many ticks?")?;
                Ok(Self::Step {
                    ticks: number(Some(ticks))?,
                    received: number(words.next())?,
                })
            }
            Some("status") => Ok(Self::Status),
            Some("stop") => Ok(Self::Stop),
            _ => Err(format!("unknown order {line}")),
        }
    }

    fn carry_out(self, app: &mut App, counters: &Counters) -> (String, Option<AppExit>) {
        match self {
            Self::Status => (report(app.world(), counters), None),
            Self::Stop => {
                app.world_mut().write_message(AppExit::Success);
                app.update();
                let exit = app.should_exit().unwrap_or(AppExit::Success);
                (report(app.world(), counters), Some(exit))
            }
            Self::Step { ticks, received } => {
                if !fence(counters, received, FENCE_LIMIT) {
                    let have = counters.received.load(Ordering::SeqCst);
                    let error = format!("fence expired: received {have} of {received}");
                    return (failure(&error, app.world(), counters), None);
                }
                for _ in 0..ticks {
                    app.update();
                    if let Some(exit) = app.should_exit() {
                        return (report(app.world(), counters), Some(exit));
                    }
                }
                (report(app.world(), counters), None)
            }
        }
    }
}

fn fence(counters: &Counters, received: u64, limit: Duration) -> bool {
    #[expect(
        clippy::disallowed_methods,
        reason = "the fence of the stepped runner waits on the wall clock, outside the game"
    )]
    let start = Instant::now();
    while counters.received.load(Ordering::SeqCst) < received {
        if start.elapsed() > limit {
            return false;
        }
        sleep(FENCE_POLL);
    }
    true
}

fn fields(world: &World, counters: &Counters) -> String {
    format!(
        "\"tick\":{},\"sent\":{},\"received\":{}",
        crate::pace::of(world).ticks_started(),
        counters.sent.load(Ordering::SeqCst),
        counters.received.load(Ordering::SeqCst)
    )
}

fn report(world: &World, counters: &Counters) -> String {
    format!("{{\"ok\":true,{}}}", fields(world, counters))
}

fn failure(error: &str, world: &World, counters: &Counters) -> String {
    format!(
        "{{\"ok\":false,\"error\":{},{}}}",
        json_string(error),
        fields(world, counters)
    )
}

fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use bevy_app::Update;
    use bevy_ecs::prelude::*;

    use super::*;

    #[derive(Resource, Default)]
    struct Updates(u64);

    fn count(mut updates: ResMut<Updates>) {
        updates.0 += 1;
    }

    fn app() -> App {
        let mut app = App::new();
        app.init_resource::<Updates>().add_systems(Update, count);
        app
    }

    fn updates(app: &App) -> u64 {
        app.world().resource::<Updates>().0
    }

    #[test]
    fn orders_are_read_from_their_line() {
        assert_eq!(
            Order::parse("step 3 40"),
            Ok(Order::Step {
                ticks: 3,
                received: 40
            })
        );
        assert_eq!(
            Order::parse("step 1"),
            Ok(Order::Step {
                ticks: 1,
                received: 0
            })
        );
        assert_eq!(Order::parse("status"), Ok(Order::Status));
        assert_eq!(Order::parse("stop"), Ok(Order::Stop));
        assert!(Order::parse("step").is_err());
        assert!(Order::parse("step many").is_err());
        assert!(Order::parse("jump").is_err());
    }

    #[test]
    fn a_step_runs_exactly_the_ticks_asked_and_status_runs_none() {
        let mut app = app();
        let counters = Counters::default();
        Order::Status.carry_out(&mut app, &counters);
        assert_eq!(updates(&app), 0);
        let order = Order::Step {
            ticks: 5,
            received: 0,
        };
        let (reply, exit) = order.carry_out(&mut app, &counters);
        assert_eq!(updates(&app), 5);
        assert!(reply.starts_with("{\"ok\":true"));
        assert!(exit.is_none());
    }

    #[test]
    fn a_step_waits_for_the_packets_of_the_client() {
        let counters = Arc::new(Counters::default());
        let late = counters.clone();
        let arrival = std::thread::spawn(move || {
            sleep(Duration::from_millis(30));
            late.received.store(2, Ordering::SeqCst);
        });
        #[expect(
            clippy::disallowed_methods,
            reason = "the test waits on the wall clock"
        )]
        let start = Instant::now();
        assert!(fence(&counters, 2, Duration::from_secs(5)));
        assert!(start.elapsed() >= Duration::from_millis(25));
        arrival.join().unwrap();
    }

    #[test]
    fn a_fence_expires_when_the_packets_never_come() {
        let counters = Counters::default();
        counters.received.store(1, Ordering::SeqCst);
        assert!(!fence(&counters, 2, Duration::from_millis(5)));
    }

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("void-step-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn connect(socket: &Path) -> UnixStream {
        #[expect(
            clippy::disallowed_methods,
            reason = "the test waits on the wall clock"
        )]
        let start = Instant::now();
        loop {
            if let Ok(stream) = UnixStream::connect(socket) {
                return stream;
            }
            assert!(start.elapsed() < Duration::from_secs(5));
            sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn a_socket_with_a_server_behind_it_is_not_taken() {
        let dir = folder("taken");
        let socket = dir.join("server.sock");
        let _first = UnixListener::bind(&socket).unwrap();
        let path = socket.clone();
        let (done, exit) = flume::bounded(1);
        std::thread::spawn(move || {
            let _ = done.send(run(app(), &path, &Counters::default()));
        });
        let exit = exit.recv_timeout(Duration::from_secs(5));
        assert!(exit.expect("the socket was taken").is_error());
        assert!(UnixStream::connect(&socket).is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_stale_socket_is_replaced() {
        let dir = folder("stale");
        let socket = dir.join("server.sock");
        drop(UnixListener::bind(&socket).unwrap());
        let path = socket.clone();
        let runner = std::thread::spawn(move || run(app(), &path, &Counters::default()));
        let mut stream = connect(&socket);
        writeln!(stream, "stop").unwrap();
        assert!(runner.join().unwrap().is_success());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_socket_that_cannot_be_bound_is_an_error() {
        let socket = std::env::temp_dir().join("void-step-no-such-folder/server.sock");
        assert!(run(app(), &socket, &Counters::default()).is_error());
    }

    #[derive(Resource, Default)]
    struct Seen(bool);

    fn watch_exit(mut exits: MessageReader<AppExit>, mut seen: ResMut<Seen>) {
        seen.0 |= exits.read().next().is_some();
    }

    #[test]
    fn stop_goes_through_app_exit() {
        let mut app = app();
        app.init_resource::<Seen>().add_systems(Update, watch_exit);
        let (_, exit) = Order::Stop.carry_out(&mut app, &Counters::default());
        assert_eq!(exit, Some(AppExit::Success));
        assert!(app.world().resource::<Seen>().0);
    }

    #[test]
    fn a_failure_is_always_valid_json() {
        assert_eq!(json_string("a\"b\\\u{1}é"), "\"a\\\"b\\\\\\u0001é\"");
    }

    #[test]
    fn the_relay_counts_a_packet_once_the_game_can_read_it() {
        let counters = Arc::new(Counters::default());
        let (game, read) = flume::unbounded();
        let network = relay(game, counters.clone());
        network.send(7).unwrap();
        assert_eq!(read.recv_timeout(Duration::from_secs(5)), Ok(7));
        assert!(fence(&counters, 1, Duration::from_secs(5)));
    }

    #[test]
    fn the_plugin_runs_the_app_and_numbers_its_ticks() {
        let dir = folder("plugin");
        let socket = dir.join("server.sock");
        let path = socket.clone();
        let runner = std::thread::spawn(move || {
            let mut app = app();
            app.add_plugins(StepPlugin::new(
                path,
                Duration::from_millis(50),
                Arc::default(),
            ));
            app.run()
        });
        let stream = connect(&socket);
        let mut lines = BufReader::new(stream.try_clone().unwrap()).lines();
        let mut stream = stream;
        let mut tick = || {
            let line = lines.next().unwrap().unwrap();
            let rest = line.split("\"tick\":").nth(1).unwrap().to_string();
            rest.split(',').next().unwrap().parse::<u64>().unwrap()
        };
        writeln!(stream, "status").unwrap();
        let before = tick();
        writeln!(stream, "step 3").unwrap();
        assert!(tick() >= before + 3);
        writeln!(stream, "stop").unwrap();
        tick();
        assert!(runner.join().unwrap().is_success());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stop_ends_the_runner_and_removes_the_socket() {
        let dir = folder("stop");
        let socket = dir.join("server.sock");
        let path = socket.clone();
        let runner = std::thread::spawn(move || {
            let counters = Counters::default();
            let exit = run(app(), &path, &counters);
            exit == AppExit::Success
        });
        let stream = connect(&socket);
        let mut lines = BufReader::new(stream.try_clone().unwrap()).lines();
        let mut stream = stream;
        writeln!(stream, "step 2").unwrap();
        assert!(lines.next().unwrap().unwrap().contains("\"ok\":true"));
        writeln!(stream, "fly").unwrap();
        assert!(lines.next().unwrap().unwrap().contains("unknown order"));
        writeln!(stream, "stop").unwrap();
        lines.next().unwrap().unwrap();
        assert!(runner.join().unwrap());
        assert!(!socket.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
