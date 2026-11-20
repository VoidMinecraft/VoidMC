//! Runs the built `voidmc-example` binary with its diagnostics variables and
//! checks the files it leaves behind once stopped with Ctrl-C (SIGINT): the
//! TPS CSV (`VOID_METRICS_DEBUG`, `VOID_TPS_OUTPUT`) and the folded flame
//! trace (`VOID_METRICS_MODE=flame`, `VOID_FLAME_OUTPUT`,
//! `VOID_FLAME_INCLUDE_IDLE`). It also checks the exit code of a server
//! stopped by Ctrl-C or SIGTERM, and that the `/health` and `/metrics`
//! endpoints (`VOID_OBSERVABILITY_ADDR`) serve while all of these are on.
//!
//! Each run listens on a free port (`VOID_LISTEN_ADDR=127.0.0.1:0`) inside its
//! own temporary directory, so relative paths and the default `logs/` paths
//! resolve there and the directory is removed afterwards.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Upper bound on each wait; only keeps a hung server from hanging the run.
const WAIT_LIMIT: Duration = Duration::from_secs(120);

const TPS_HEADER: &str = "timestamp_ms,tps,window_ms,last_tick_ms,total_ticks";

/// The example's tick rate: 20 ticks per second.
const TICK_RATE: f64 = 20.0;

/// The exit code of a server stopped by Ctrl-C, SIGTERM or SIGHUP: Bevy's
/// handler for these signals ends the app with `AppExit::from_code(130)`, the
/// shell's code for a process interrupted by SIGINT (128 + 2).
const INTERRUPTED: i32 = 130;

/// Every variable the example reads; each run starts without them.
const VARIABLES: [&str; 9] = [
    "RUST_LOG",
    "VOID_LISTEN_ADDR",
    "VOID_OBSERVABILITY_ADDR",
    "VOID_METRICS_DEBUG",
    "VOID_TPS_OUTPUT",
    "VOID_METRICS_MODE",
    "VOID_FLAME_OUTPUT",
    "VOID_FLAME_INCLUDE_IDLE",
    "VOID_PACKET_DEBUG",
];

#[test]
fn tps_csv_and_flame_trace_are_complete_after_ctrl_c() {
    let started_ms = unix_ms();
    let mut server = Example::start(
        "enabled",
        |dir| {
            // An earlier run's file is replaced, not appended to.
            std::fs::create_dir(dir.join("stats")).unwrap();
            std::fs::write(dir.join("stats/tps.csv"), format!("{TPS_HEADER}\nstale\n")).unwrap();
        },
        &[
            ("VOID_METRICS_DEBUG", "1"),
            ("VOID_TPS_OUTPUT", "stats/tps.csv"),
            ("VOID_METRICS_MODE", "flame"),
            // The parent directories do not exist yet.
            ("VOID_FLAME_OUTPUT", "profiles/run/trace.folded"),
        ],
    );
    server.expect_line("the TPS output line", |line| {
        line.contains("Writing TPS samples to stats/tps.csv")
    });
    server.expect_line("the flame output line", |line| {
        line.contains("Writing flame trace to profiles/run/trace.folded")
    });
    server.wait_for_samples(2, 20);

    let stopped = server.interrupt();
    let finished_ms = unix_ms();

    let samples = stopped.logged_samples();
    let rows = tps_rows(&stopped.read("stats/tps.csv"));
    assert_eq!(
        rows.iter().map(|row| row.total_ticks).collect::<Vec<_>>(),
        samples,
        "one row per logged sample, the last one included"
    );
    assert!(rows.last().unwrap().total_ticks >= 20);
    check_tps_rows(&rows, started_ms, finished_ms);

    let stacks = folded_stacks(&stopped.read("profiles/run/trace.folded"));
    // `ingest_network_packets` runs once per tick and writes this line when
    // it ends. Each tick the CSV counted must be in the trace, which holds
    // only if the trace was flushed at exit: a lost write buffer drops the
    // last dozen or so ticks.
    let traced_ticks = stacks
        .iter()
        .filter(|stack| *stack == &["all-threads", "ingest_network_packets"])
        .count() as u64;
    let counted_ticks = rows.last().unwrap().total_ticks;
    assert!(
        traced_ticks >= counted_ticks,
        "the trace holds {traced_ticks} ticks of the at least {counted_ticks} run"
    );
    assert!(
        stacks
            .iter()
            .any(|stack| stack.iter().any(|frame| frame == "ingest_network_packets")),
        "the trace holds the server's instrumented systems: {stacks:?}"
    );
    assert!(
        !stacks.iter().any(|stack| stack.len() == 1),
        "idle samples are left out by default: {stacks:?}"
    );
}

/// `VOID_OBSERVABILITY_ADDR` together with every diagnostics variable: the
/// endpoints report a healthy server whose tick counter keeps up with the
/// TPS samples, and after Ctrl-C the CSV and the trace are as complete as
/// without them, and the exit code is 130.
#[test]
fn observability_serves_while_the_tps_csv_and_flame_trace_are_written() {
    let started_ms = unix_ms();
    let mut server = Example::start(
        "observed",
        |_| {},
        &[
            ("VOID_OBSERVABILITY_ADDR", "127.0.0.1:0"),
            ("VOID_METRICS_DEBUG", "1"),
            ("VOID_TPS_OUTPUT", "stats/tps.csv"),
            ("VOID_METRICS_MODE", "flame"),
            ("VOID_FLAME_OUTPUT", "profiles/trace.folded"),
        ],
    );
    let listening = server.expect_line("the observability address", |line| {
        line.contains("Observability endpoints listening")
    });
    let address = listen_addr(&listening);
    assert!(address.ip().is_loopback(), "{listening}");
    server.expect_line("the TPS output line", |line| {
        line.contains("Writing TPS samples to stats/tps.csv")
    });
    server.expect_line("the flame output line", |line| {
        line.contains("Writing flame trace to profiles/trace.folded")
    });

    let deadline = Instant::now() + WAIT_LIMIT;
    let health = loop {
        let (status, body) = http_get(address, "/health");
        if status == 200 {
            break body;
        }
        assert!(Instant::now() < deadline, "never healthy: {status} {body}");
        thread::sleep(Duration::from_millis(50));
    };
    assert!(health.contains("\"status\":\"ok\""), "{health}");

    server.wait_for_samples(2, 20);
    let sampled = server
        .lines
        .iter()
        .rev()
        .find_map(|line| sample_ticks(line));
    let sampled = sampled.expect("a TPS sample");
    let (status, metrics) = http_get(address, "/metrics");
    assert_eq!(status, 200, "{metrics}");
    let scraped = metric(&metrics, "voidmc_ticks_total");
    // The sample is logged during its tick, which the counter counts once
    // it has ended.
    assert!(
        scraped + 1.0 >= sampled as f64,
        "the endpoint counts {scraped} ticks, the last TPS sample {sampled}"
    );
    assert_eq!(
        metric(&metrics, "voidmc_target_ticks_per_second"),
        TICK_RATE
    );
    assert!(metric(&metrics, "voidmc_tick_duration_seconds_count") >= scraped);

    let stopped = server.interrupt();
    let finished_ms = unix_ms();

    let rows = tps_rows(&stopped.read("stats/tps.csv"));
    assert_eq!(
        rows.iter().map(|row| row.total_ticks).collect::<Vec<_>>(),
        stopped.logged_samples(),
        "one row per logged sample, the last one included"
    );
    check_tps_rows(&rows, started_ms, finished_ms);
    let stacks = folded_stacks(&stopped.read("profiles/trace.folded"));
    let traced_ticks = stacks
        .iter()
        .filter(|stack| *stack == &["all-threads", "ingest_network_packets"])
        .count() as u64;
    let counted_ticks = rows.last().unwrap().total_ticks;
    assert!(
        traced_ticks >= counted_ticks,
        "the trace holds {traced_ticks} ticks of the at least {counted_ticks} run"
    );
}

/// The address of the `listen_addr: <address>` field of a log line.
fn listen_addr(line: &str) -> SocketAddr {
    let (_, rest) = line
        .split_once("listen_addr: ")
        .unwrap_or_else(|| panic!("no listen_addr in {line:?}"));
    let address = rest.split_whitespace().next().unwrap_or_default();
    address
        .parse()
        .unwrap_or_else(|_| panic!("not an address: {address:?} in {line:?}"))
}

/// One `GET` on an endpoint, which answers and closes the connection.
fn http_get(address: SocketAddr, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect_timeout(&address, WAIT_LIMIT)
        .unwrap_or_else(|error| panic!("connect to {address}: {error}"));
    stream.set_read_timeout(Some(WAIT_LIMIT)).unwrap();
    write!(stream, "GET {path} HTTP/1.1\r\nHost: test\r\n\r\n").unwrap();
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .unwrap_or_else(|error| panic!("GET {path}: {error}"));
    let (head, body) = response
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("no end of headers in {response:?}"));
    let status = head
        .split(' ')
        .nth(1)
        .and_then(|status| status.parse().ok())
        .unwrap_or_else(|| panic!("no status in {head:?}"));
    (status, body.to_string())
}

/// The value of the sample `name`, without labels, in a Prometheus text body.
fn metric(body: &str, name: &str) -> f64 {
    body.lines()
        .find_map(|line| line.strip_prefix(name)?.strip_prefix(' '))
        .unwrap_or_else(|| panic!("no {name} sample in\n{body}"))
        .parse()
        .unwrap_or_else(|_| panic!("{name} is not a number in\n{body}"))
}

#[test]
fn ctrl_c_exits_with_code_130() {
    let mut server = Example::start("ctrl-c", |_| {}, &[]);
    server.expect_line("the spawn area log line", |line| {
        line.contains("spawn area chunks")
    });

    let stopped = server.stop_with("INT");

    stopped.assert_exit_code(INTERRUPTED);
}

#[test]
fn sigterm_exits_with_code_130() {
    let mut server = Example::start("sigterm", |_| {}, &[]);
    server.expect_line("the spawn area log line", |line| {
        line.contains("spawn area chunks")
    });

    let stopped = server.stop_with("TERM");

    stopped.assert_exit_code(INTERRUPTED);
}

#[test]
fn nothing_is_written_when_diagnostics_are_off() {
    let mut server = Example::start("disabled", |_| {}, &[]);
    // The TPS file and the trace file are opened before the first tick.
    server.expect_line("the spawn area log line", |line| {
        line.contains("spawn area chunks")
    });

    let stopped = server.interrupt();

    let diagnostics =
        stopped.files_matching(|name| name.ends_with(".csv") || name.ends_with(".folded"));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    stopped.assert_no_line(|line| line.contains("TPS sample") || line.contains("Writing flame"));
}

#[test]
fn tps_alone_writes_the_default_csv_and_no_trace() {
    let mut server = Example::start("tps-only", |_| {}, &[("VOID_METRICS_DEBUG", "1")]);
    server.wait_for_samples(1, 1);

    let stopped = server.interrupt();

    let csv = stopped.files_matching(|name| name.ends_with(".csv"));
    assert_eq!(csv.len(), 1, "{csv:?}");
    let name = csv[0].to_str().unwrap();
    let stamp = name
        .strip_prefix("logs/tps-")
        .and_then(|rest| rest.strip_suffix(".csv"))
        .unwrap_or_else(|| panic!("not the default logs/tps-<timestamp>.csv: {name}"));
    stamp.parse::<u128>().unwrap();
    let rows = tps_rows(&stopped.read(name));
    assert_eq!(rows.len(), stopped.logged_samples().len());

    let traces = stopped.files_matching(|name| name.ends_with(".folded"));
    assert!(traces.is_empty(), "{traces:?}");
}

#[test]
fn flame_alone_writes_the_default_trace_with_idle_time_and_no_csv() {
    let mut server = Example::start(
        "flame-only",
        |_| {},
        &[
            ("VOID_METRICS_MODE", "flame"),
            ("VOID_FLAME_INCLUDE_IDLE", "1"),
        ],
    );
    server.expect_line("the flame output line", |line| {
        line.contains("Writing flame trace to logs/trace-")
    });
    server.expect_line("the spawn area log line", |line| {
        line.contains("spawn area chunks")
    });

    let stopped = server.interrupt();

    let traces = stopped.files_matching(|name| name.ends_with(".folded"));
    assert_eq!(traces.len(), 1, "{traces:?}");
    let name = traces[0].to_str().unwrap();
    assert!(
        name.starts_with("logs/trace-") && name.ends_with(".folded"),
        "not the default logs/trace-<timestamp>.folded: {name}"
    );
    let stacks = folded_stacks(&stopped.read(name));
    assert!(
        stacks.iter().any(|stack| stack.len() == 1),
        "VOID_FLAME_INCLUDE_IDLE=1 records idle samples: {stacks:?}"
    );
    assert!(
        stacks
            .iter()
            .any(|stack| stack.iter().any(|frame| frame == "load_or_generate")),
        "{stacks:?}"
    );

    let csv = stopped.files_matching(|name| name.ends_with(".csv"));
    assert!(
        csv.is_empty(),
        "flame mode alone writes no TPS CSV: {csv:?}"
    );
    stopped.assert_no_line(|line| line.contains("TPS sample"));
}

#[test]
fn unusable_output_paths_are_warnings_and_the_server_runs_on() {
    let mut server = Example::start(
        "unusable",
        |dir| {
            std::fs::create_dir(dir.join("taken")).unwrap();
            std::fs::write(dir.join("plain"), "a file").unwrap();
        },
        &[
            ("VOID_METRICS_DEBUG", "1"),
            // A directory, and a path under a regular file.
            ("VOID_TPS_OUTPUT", "taken"),
            ("VOID_FLAME_OUTPUT", "plain/trace.folded"),
        ],
    );
    let tps_warning = server.expect_line("the TPS output warning", |line| {
        line.contains(" WARN ") && line.contains("path: taken")
    });
    let flame_warning = server.expect_line("the flame output warning", |line| {
        line.contains(" WARN ") && line.contains("path: plain/trace.folded")
    });
    for warning in [&tps_warning, &flame_warning] {
        assert!(warning.contains("error: "), "names the reason: {warning}");
    }
    // The server runs on and keeps sampling (to the log only).
    server.wait_for_samples(2, 20);

    let stopped = server.interrupt();

    stopped.assert_no_line(|line| {
        line.contains("Writing TPS samples")
            || line.contains("Writing flame trace")
            || line.contains("panicked")
            || line.contains(" ERROR ")
    });
    assert_eq!(
        std::fs::read_to_string(stopped.dir.0.join("plain")).unwrap(),
        "a file"
    );
    assert_eq!(
        std::fs::read_dir(stopped.dir.0.join("taken"))
            .unwrap()
            .count(),
        0
    );
    let diagnostics =
        stopped.files_matching(|name| name.ends_with(".csv") || name.ends_with(".folded"));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

struct TpsRow {
    timestamp_ms: u128,
    tps: f64,
    window_ms: f64,
    last_tick_ms: f64,
    total_ticks: u64,
}

/// Parses a TPS CSV: exactly one header line, then complete rows.
fn tps_rows(content: &str) -> Vec<TpsRow> {
    assert!(
        content.ends_with('\n'),
        "the last line is complete: {content:?}"
    );
    let mut lines = content.lines();
    assert_eq!(lines.next(), Some(TPS_HEADER), "{content:?}");
    let rows: Vec<TpsRow> = lines
        .map(|line| {
            let columns: Vec<&str> = line.split(',').collect();
            assert_eq!(columns.len(), 5, "a five-column row: {line:?}");
            let float = |index: usize| -> f64 {
                columns[index]
                    .parse()
                    .unwrap_or_else(|_| panic!("column {index} is not a number: {line:?}"))
            };
            TpsRow {
                timestamp_ms: columns[0]
                    .parse()
                    .unwrap_or_else(|_| panic!("not a millisecond timestamp: {line:?}")),
                tps: float(1),
                window_ms: float(2),
                last_tick_ms: float(3),
                total_ticks: columns[4]
                    .parse()
                    .unwrap_or_else(|_| panic!("not a tick count: {line:?}")),
            }
        })
        .collect();
    assert!(!rows.is_empty(), "{content:?}");
    rows
}

/// Checks the columns against each other and against the wall clock.
fn check_tps_rows(rows: &[TpsRow], started_ms: u128, finished_ms: u128) {
    let mut previous: Option<&TpsRow> = None;
    for row in rows {
        assert!(
            (started_ms..=finished_ms).contains(&row.timestamp_ms),
            "the timestamp is taken during the run"
        );
        // A sample closes a window of at least one second.
        assert!(row.window_ms >= 1000.0, "window_ms {}", row.window_ms);
        assert!(
            row.tps > 0.0 && row.tps <= TICK_RATE * 1.1,
            "tps {}",
            row.tps
        );
        assert!(
            row.last_tick_ms > 0.0 && row.last_tick_ms <= row.window_ms,
            "last_tick_ms {}",
            row.last_tick_ms
        );
        let (ticks_before, time_before) =
            previous.map_or((0, None), |p| (p.total_ticks, Some(p.timestamp_ms)));
        let ticks = row.total_ticks - ticks_before;
        assert!(ticks > 0, "total_ticks increases");
        let expected_tps = ticks as f64 / (row.window_ms / 1000.0);
        assert!(
            (row.tps - expected_tps).abs() < 0.02,
            "tps {} is the window's {ticks} ticks over {} ms",
            row.tps,
            row.window_ms
        );
        if let Some(time_before) = time_before {
            let elapsed = (row.timestamp_ms - time_before) as f64;
            assert!(
                (elapsed - row.window_ms).abs() < 50.0,
                "timestamps advance by the window: {elapsed} ms vs {} ms",
                row.window_ms
            );
        }
        previous = Some(row);
    }
}

/// Parses a folded trace as `inferno-flamegraph` reads it: one
/// `frame;frame;... <count>` line per stack. Returns the stacks' frames.
fn folded_stacks(content: &str) -> Vec<Vec<String>> {
    assert!(!content.is_empty(), "the trace is not empty");
    assert!(content.ends_with('\n'), "the last line is complete");
    content
        .lines()
        .map(|line| {
            let (stack, count) = line
                .rsplit_once(' ')
                .unwrap_or_else(|| panic!("not `stack count`: {line:?}"));
            count
                .parse::<u64>()
                .unwrap_or_else(|_| panic!("not a sample count: {line:?}"));
            let frames: Vec<String> = stack.split(';').map(|f| f.trim().to_string()).collect();
            assert!(
                frames.iter().all(|frame| !frame.is_empty()),
                "an empty frame: {line:?}"
            );
            assert_eq!(frames[0], "all-threads", "{line:?}");
            frames
        })
        .collect()
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

/// The example binary running in its own temporary directory. Dropping it
/// kills the process and removes the directory, also when the test fails.
struct Example {
    child: Child,
    dir: RuntimeDir,
    output: Receiver<String>,
    lines: Vec<String>,
}

impl Example {
    fn start(name: &str, prepare: impl FnOnce(&Path), vars: &[(&str, &str)]) -> Self {
        let dir = RuntimeDir::new(name);
        prepare(&dir.0);

        let mut command = Command::new(env!("CARGO_BIN_EXE_voidmc-example"));
        for variable in VARIABLES {
            command.env_remove(variable);
        }
        let mut child = command
            .envs(vars.iter().copied())
            .env("VOID_LISTEN_ADDR", "127.0.0.1:0")
            .current_dir(&dir.0)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start the voidmc-example binary");

        let (sender, output) = mpsc::channel();
        forward_lines(child.stdout.take().expect("piped stdout"), sender.clone());
        forward_lines(child.stderr.take().expect("piped stderr"), sender);

        Self {
            child,
            dir,
            output,
            lines: Vec::new(),
        }
    }

    /// Returns the first new output line, ANSI colours removed, that
    /// `matches`, failing if the process exits or `WAIT_LIMIT` passes first.
    fn wait_for_line(&mut self, what: &str, matches: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + WAIT_LIMIT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.output.recv_timeout(remaining) {
                Ok(line) => {
                    self.lines.push(line.clone());
                    if matches(&line) {
                        return line;
                    }
                }
                Err(RecvTimeoutError::Timeout) => self.fail(&format!(
                    "timed out after {WAIT_LIMIT:?} waiting for {what}"
                )),
                Err(RecvTimeoutError::Disconnected) => {
                    self.fail(&format!("the server closed its output before {what}"))
                }
            }
        }
    }

    /// Returns the first output line so far that `matches`, waiting for it
    /// if it has not been printed yet.
    fn expect_line(&mut self, what: &str, matches: impl Fn(&str) -> bool) -> String {
        match self.lines.iter().find(|line| matches(line)) {
            Some(line) => line.clone(),
            None => self.wait_for_line(what, matches),
        }
    }

    /// Waits for `count` more logged TPS samples, the last one at least
    /// `ticks` ticks into the run.
    fn wait_for_samples(&mut self, count: usize, ticks: u64) {
        for _ in 0..count {
            self.wait_for_line("a TPS sample", |line| sample_ticks(line).is_some());
        }
        while self.lines.iter().rev().find_map(|line| sample_ticks(line)) < Some(ticks) {
            self.wait_for_line("a later TPS sample", |line| sample_ticks(line).is_some());
        }
    }

    /// Sends SIGINT, as Ctrl-C does, and waits for the server to exit as
    /// an interrupted process does, with code 130.
    fn interrupt(self) -> Stopped {
        let stopped = self.stop_with("INT");
        stopped.assert_exit_code(INTERRUPTED);
        stopped
    }

    /// Sends the signal `name` (`INT`, `TERM`, ...) and waits for the process
    /// to exit, whatever its exit status.
    fn stop_with(mut self, name: &str) -> Stopped {
        let status = Command::new("kill")
            .args([&format!("-{name}"), &self.child.id().to_string()])
            .status()
            .expect("run kill");
        assert!(status.success(), "kill -{name} failed: {status}");

        // Both pipes close when the process exits.
        let deadline = Instant::now() + WAIT_LIMIT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.output.recv_timeout(remaining) {
                Ok(line) => self.lines.push(line),
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    self.fail(&format!("the server did not stop within {WAIT_LIMIT:?}"))
                }
            }
        }
        let status = self.child.wait().expect("wait for the server process");
        let lines = std::mem::take(&mut self.lines);
        let dir = RuntimeDir(std::mem::take(&mut self.dir.0));
        Stopped { lines, dir, status }
    }

    /// Stops the server and fails the test with its whole output.
    fn fail(&mut self, message: &str) -> ! {
        let _ = self.child.kill();
        let status = self.child.wait().expect("wait for the server process");
        self.lines.extend(self.output.try_iter());
        panic!(
            "{message}\n--- voidmc-example output ({status}) ---\n{}",
            self.lines.join("\n")
        );
    }
}

impl Drop for Example {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A stopped run: its output, the directory it ran in and how it exited.
struct Stopped {
    lines: Vec<String>,
    dir: RuntimeDir,
    status: ExitStatus,
}

impl Stopped {
    fn assert_exit_code(&self, code: i32) {
        assert_eq!(
            self.status.code(),
            Some(code),
            "the server exited with {}, not code {code}\n{}",
            self.status,
            self.transcript()
        );
    }

    fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.dir.0.join(relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}\n{}", self.transcript()))
    }

    /// The `total_ticks` of every logged TPS sample, in order.
    fn logged_samples(&self) -> Vec<u64> {
        self.lines
            .iter()
            .filter_map(|line| sample_ticks(line))
            .collect()
    }

    /// Every file under the run directory whose name `matches`, relative.
    fn files_matching(&self, matches: impl Fn(&str) -> bool) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut pending = vec![self.dir.0.clone()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                } else if matches(path.file_name().unwrap().to_str().unwrap()) {
                    found.push(path.strip_prefix(&self.dir.0).unwrap().to_path_buf());
                }
            }
        }
        found
    }

    fn assert_no_line(&self, forbidden: impl Fn(&str) -> bool) {
        if let Some(line) = self.lines.iter().find(|line| forbidden(line)) {
            panic!("unexpected output line {line:?}\n{}", self.transcript());
        }
    }

    fn transcript(&self) -> String {
        format!("--- voidmc-example output ---\n{}", self.lines.join("\n"))
    }
}

/// `total_ticks` of a `TPS sample` log line.
fn sample_ticks(line: &str) -> Option<u64> {
    if !line.contains("voidmc::metrics: TPS sample") {
        return None;
    }
    line.rsplit_once("total_ticks: ")?.1.trim().parse().ok()
}

/// A directory under Cargo's per-target temporary directory, removed when
/// dropped.
struct RuntimeDir(PathBuf);

impl RuntimeDir {
    fn new(name: &str) -> Self {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "voidmc-example-diagnostics-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the runtime directory");
        Self(dir)
    }
}

impl Drop for RuntimeDir {
    fn drop(&mut self) {
        if !self.0.as_os_str().is_empty() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

fn forward_lines(stream: impl Read + Send + 'static, sender: mpsc::Sender<String>) {
    thread::spawn(move || {
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { break };
            if sender.send(strip_ansi(&line)).is_err() {
                break;
            }
        }
    });
}

/// Removes the colour escape sequences the console logger writes even to a
/// pipe.
fn strip_ansi(line: &str) -> String {
    let mut plain = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.next() == Some('[') {
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
        } else {
            plain.push(c);
        }
    }
    plain
}
