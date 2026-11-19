//! VMC-ATP-025: the TPS CSV a server writes with `metrics_debug(true)` and
//! `metrics_tps_output(path)`, and what it does without them or with an
//! unusable path. The example binary's environment variables and flame
//! traces are covered by `void-example/tests/diagnostics.rs`.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use bevy_app::PostUpdate;
use bevy_ecs::schedule::{ScheduleError, Schedules, SystemSet};
use bevy_ecs::world::World;
use tracing::Level;
use voidmc::VoidSystems;
use voidmc_e2e::TestServer;

const HEADER: &str = "timestamp_ms,tps,window_ms,last_tick_ms,total_ticks";

/// The `TestServer` tick rate.
const TICK_RATE: f64 = 50.0;

/// A directory under the system temp dir, removed when dropped (also when
/// the test fails).
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("voidmc-e2e-atp025-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

/// Whether the TPS tracking system is scheduled, the only system in
/// `VoidSystems::Metrics`.
fn samples_tps(world: &mut World) -> bool {
    let schedule = world
        .resource::<Schedules>()
        .get(PostUpdate)
        .expect("a PostUpdate schedule");
    match schedule
        .graph()
        .systems_in_set(VoidSystems::Metrics.intern())
    {
        Ok(systems) => !systems.is_empty(),
        Err(ScheduleError::SetNotFound) => false,
        Err(error) => panic!("cannot inspect PostUpdate: {error}"),
    }
}

fn data_rows(path: &Path) -> usize {
    std::fs::read_to_string(path)
        .map(|content| content.lines().skip(1).count())
        .unwrap_or(0)
}

#[tokio::test]
async fn tps_csv_has_the_header_and_a_row_per_second() {
    let dir = TempDir::new("enabled");
    // The parent directory does not exist yet.
    let path = dir.0.join("metrics/tps.csv");
    let output = path.display().to_string();
    let started_ms = unix_ms();
    let server = TestServer::builder()
        .config(|config| config.metrics_debug(true).metrics_tps_output(output))
        .start()
        .await;

    assert!(server.with_world(samples_tps).await);
    server.wait_ticks(20).await;
    let watched = path.clone();
    server
        .wait_for("two TPS samples", move |_| {
            (data_rows(&watched) >= 2).then_some(())
        })
        .await;
    server.stop().await;
    let finished_ms = unix_ms();

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(
        content.ends_with('\n'),
        "the last row is complete: {content:?}"
    );
    let mut lines = content.lines();
    assert_eq!(lines.next(), Some(HEADER));
    let mut previous: Option<(u128, u64)> = None;
    let mut rows = 0;
    for line in lines {
        rows += 1;
        let columns: Vec<&str> = line.split(',').collect();
        assert_eq!(columns.len(), 5, "{line:?}");
        let timestamp_ms: u128 = columns[0].parse().unwrap();
        let tps: f64 = columns[1].parse().unwrap();
        let window_ms: f64 = columns[2].parse().unwrap();
        let last_tick_ms: f64 = columns[3].parse().unwrap();
        let total_ticks: u64 = columns[4].parse().unwrap();

        assert!(
            (started_ms..=finished_ms).contains(&timestamp_ms),
            "{line:?}"
        );
        assert!(window_ms >= 1000.0, "a window lasts a second: {line:?}");
        assert!(tps > 0.0 && tps <= TICK_RATE * 1.1, "{line:?}");
        assert!(last_tick_ms > 0.0 && last_tick_ms <= window_ms, "{line:?}");
        let (time_before, ticks_before) =
            previous.map_or((None, 0), |(time, ticks)| (Some(time), ticks));
        assert!(total_ticks > ticks_before, "{line:?}");
        let expected_tps = (total_ticks - ticks_before) as f64 / (window_ms / 1000.0);
        assert!(
            (tps - expected_tps).abs() < 0.02,
            "tps is the window's ticks per second: {line:?}"
        );
        if let Some(time_before) = time_before {
            let elapsed = (timestamp_ms - time_before) as f64;
            assert!(
                (elapsed - window_ms).abs() < 50.0,
                "timestamps advance by the window: {line:?}"
            );
        }
        previous = Some((timestamp_ms, total_ticks));
    }
    assert!(rows >= 2, "{content:?}");
    assert!(previous.unwrap().1 >= 20, "{content:?}");
}

#[tokio::test]
async fn no_tps_file_and_no_sampling_without_metrics_debug() {
    let dir = TempDir::new("disabled");
    let path = dir.0.join("metrics/tps.csv");
    let output = path.display().to_string();
    let server = TestServer::builder()
        .config(|config| config.metrics_tps_output(output))
        .start()
        .await;

    server.wait_ticks(60).await;
    assert!(!server.with_world(samples_tps).await);
    server.stop().await;

    assert!(!dir.0.join("metrics").exists());
}

#[tokio::test]
async fn an_unusable_tps_path_is_a_warning_and_the_server_runs_on() {
    let dir = TempDir::new("unusable");
    // A directory cannot be opened as the CSV file.
    let output = dir.0.display().to_string();
    let server = TestServer::builder()
        .config(|config| config.metrics_debug(true).metrics_tps_output(output))
        .start()
        .await;

    let warnings: Vec<String> = server
        .logs()
        .into_iter()
        .filter(|record| record.level == Level::WARN && record.target == "voidmc::metrics")
        .map(|record| record.message)
        .collect();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].contains(&format!("path={}", dir.0.display())),
        "names the path: {warnings:?}"
    );
    assert!(
        warnings[0].contains("error="),
        "gives the reason: {warnings:?}"
    );

    // The server is up: it ticks, samples and lets a player in.
    assert!(server.with_world(samples_tps).await);
    server.wait_ticks(20).await;
    let alice = server.join("Alice").await;
    alice.sync().await;
    alice.assert_healthy();
    server.stop().await;

    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 0);
}
