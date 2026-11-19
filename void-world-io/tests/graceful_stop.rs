//! VMC-ATP-026: Ctrl-C (SIGINT), SIGTERM and SIGHUP stop a persistent server
//! gracefully.
//!
//! A signal can only be tested on a real process, so this test binary runs a
//! server in a child copy of itself (`server_process`), changes a block there,
//! sends the child the signal and waits for it to exit. Periodic saving is off,
//! so the change reaches the disk only through the shutdown flush. A second
//! child then starts over the same world directory and reports the block.
//! The `/stop` path is covered by the `voidmc-e2e` scenario
//! `stop_command_saves_dirty_chunks_and_stops_the_server`.
#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use bevy_app::{AppExit, Update};
use bevy_ecs::prelude::*;
use voidmc::{
    ChunkData, ChunkDirty, ChunkIndex, ChunkPos, DimensionId, ServerConfigBuilder,
    ServerConfigResource, VoidServer,
};
use voidmc_data::v26_1_2::blocks::GLOWSTONE;
use voidmc_world_io::{PersistenceConfig, WorldPersistencePlugin};

/// Selects what `server_process` does; unset in a normal test run.
const MODE_VAR: &str = "VOIDMC_GRACEFUL_STOP_MODE";
const WORLD_VAR: &str = "VOIDMC_GRACEFUL_STOP_WORLD";

const READY: &str = "graceful-stop: block changed";
const STOPPED: &str = "graceful-stop: server stopped";
const BLOCK: &str = "graceful-stop: block";

/// The changed block, in the spawn chunk, high above generated terrain.
const LOCAL_X: u8 = 3;
const WORLD_Y: i32 = 300;
const LOCAL_Z: u8 = 11;

const TIMEOUT: Duration = Duration::from_secs(60);

fn spawn_chunk(config: &ServerConfigResource) -> ChunkPos {
    ChunkPos::from_block(config.spawn_x, config.spawn_z)
}

/// Changes one block the way gameplay does (chunk data plus `ChunkDirty`),
/// once, then tells the parent test it may send the signal.
fn change_block(
    mut done: Local<bool>,
    index: Res<ChunkIndex>,
    config: Res<ServerConfigResource>,
    mut chunks: Query<&mut ChunkData>,
    mut commands: Commands,
) {
    if *done {
        return;
    }
    *done = true;
    let entity = index.0[&(DimensionId::Overworld, spawn_chunk(&config))];
    let previous = chunks
        .get_mut(entity)
        .expect("the spawn chunk has data")
        .set_block(LOCAL_X, WORLD_Y, LOCAL_Z, GLOWSTONE)
        .expect("y in range");
    assert_ne!(
        previous, GLOWSTONE,
        "the generator already placed the block"
    );
    commands.entity(entity).insert(ChunkDirty);
    println!("{READY}");
    std::io::stdout().flush().unwrap();
}

/// Reports the block the restarted server holds, then stops it.
fn report_block(
    index: Res<ChunkIndex>,
    config: Res<ServerConfigResource>,
    chunks: Query<&ChunkData>,
    mut exit: MessageWriter<AppExit>,
) {
    let entity = index.0[&(DimensionId::Overworld, spawn_chunk(&config))];
    let block = chunks
        .get(entity)
        .expect("the spawn chunk has data")
        .get_block(LOCAL_X, WORLD_Y, LOCAL_Z);
    println!("{BLOCK} {block:?}");
    exit.write(AppExit::Success);
}

/// The server run by the child processes. Does nothing in a normal test run.
#[test]
fn server_process() {
    let Ok(mode) = std::env::var(MODE_VAR) else {
        return;
    };
    let world = std::env::var(WORLD_VAR).expect("the parent sets the world directory");
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .without_time()
        .init();

    let config = ServerConfigBuilder::new()
        .address("127.0.0.1:0")
        .spawn_chunk_radius(1)
        .build();
    VoidServer::new(config)
        .add_plugin(move |app| {
            app.add_plugins(WorldPersistencePlugin::new(
                PersistenceConfig::new(world).save_interval_ticks(u64::MAX),
            ));
            match mode.as_str() {
                "change" => app.add_systems(Update, change_block),
                "report" => app.add_systems(Update, report_block),
                other => panic!("unknown mode {other}"),
            };
        })
        .run();
    println!("{STOPPED}");
}

/// A child server process, killed if the test fails before it exits.
struct ServerProcess {
    child: Child,
    stdout: mpsc::Receiver<String>,
}

impl ServerProcess {
    fn start(mode: &str, world: &Path) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "server_process", "--nocapture"])
            .env(MODE_VAR, mode)
            .env(WORLD_VAR, world)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to start the server process");
        let (tx, stdout) = mpsc::channel();
        let pipe = child.stdout.take().unwrap();
        std::thread::spawn(move || {
            for line in BufReader::new(pipe).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Self { child, stdout }
    }

    /// Waits for a stdout line starting with `prefix` and returns it.
    fn wait_for_line(&self, prefix: &str) -> String {
        loop {
            match self.stdout.recv_timeout(TIMEOUT) {
                Ok(line) if line.starts_with(prefix) => return line,
                Ok(_) => {}
                Err(error) => panic!("the server never printed {prefix:?}: {error}"),
            }
        }
    }

    fn signal(&self, signal: &str) {
        let status = Command::new("kill")
            .args(["-s", signal, &self.child.id().to_string()])
            .status()
            .expect("failed to run kill");
        assert!(status.success(), "kill -s {signal} failed: {status}");
    }

    /// Waits for the process to exit; returns its status, the rest of its
    /// stdout and its stderr.
    fn wait(mut self) -> (ExitStatus, Vec<String>, String) {
        let mut stderr = self.child.stderr.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            let _ = tx.send(text);
        });
        // stderr reaches EOF once the process has exited.
        let stderr = rx
            .recv_timeout(TIMEOUT)
            .expect("the server process did not exit");
        let status = self.child.wait().unwrap();
        let stdout = self.stdout.try_iter().collect();
        (status, stdout, stderr)
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn stop_with_signal_and_restart(signal: &str) {
    let world = tempfile::tempdir().unwrap();

    let server = ServerProcess::start("change", world.path());
    server.wait_for_line(READY);
    server.signal(signal);
    let (status, stdout, stderr) = server.wait();
    assert!(
        status.success(),
        "SIG{signal} did not stop the server cleanly: {status}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.iter().any(|line| line == STOPPED),
        "the server did not return from VoidServer::run after SIG{signal}: {stdout:?}\nstderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("panicked"),
        "the server panicked:\n{stderr}"
    );
    let errors: Vec<&str> = stderr
        .lines()
        .filter(|line| line.trim_start().starts_with("ERROR"))
        .collect();
    assert!(errors.is_empty(), "the server logged errors: {errors:#?}");
    assert!(
        stderr.contains("world persistence: flushed on shutdown saved=1"),
        "the shutdown flush did not save the dirty chunk:\n{stderr}"
    );

    let server = ServerProcess::start("report", world.path());
    let block = server.wait_for_line(BLOCK);
    assert_eq!(
        block,
        format!("{BLOCK} Some({GLOWSTONE})"),
        "the block changed before SIG{signal} is lost after a restart"
    );
    let (status, _, stderr) = server.wait();
    assert!(
        status.success(),
        "the restarted server failed: {status}\n{stderr}"
    );
}

#[test]
fn sigint_flushes_dirty_chunks_and_exits_cleanly() {
    stop_with_signal_and_restart("INT");
}

#[test]
fn sigterm_flushes_dirty_chunks_and_exits_cleanly() {
    stop_with_signal_and_restart("TERM");
}

#[test]
fn sighup_flushes_dirty_chunks_and_exits_cleanly() {
    stop_with_signal_and_restart("HUP");
}
