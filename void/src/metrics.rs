use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bevy_app::{App, Plugin, PostUpdate};
use bevy_ecs::prelude::*;

const DEFAULT_REPORT_INTERVAL: Duration = Duration::from_secs(1);

pub struct MetricsPlugin {
    tps_output: Option<String>,
}

impl MetricsPlugin {
    pub fn new(tps_output: Option<String>) -> Self {
        Self { tps_output }
    }
}

impl Plugin for MetricsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(TpsMetrics::new(self.tps_output.clone()))
            .add_systems(PostUpdate, track_tps);
    }
}

#[derive(Resource)]
pub struct TpsMetrics {
    report_interval: Duration,
    window_start: Instant,
    window_ticks: u64,
    total_ticks: u64,
    last_tick: Instant,
    last_tick_ms: f64,
    writer: Option<BufWriter<File>>,
}

impl TpsMetrics {
    fn new(tps_output: Option<String>) -> Self {
        let now = Instant::now();
        let path = resolve_tps_path(tps_output);
        let writer = path.as_deref().and_then(open_tps_writer);

        Self {
            report_interval: DEFAULT_REPORT_INTERVAL,
            window_start: now,
            window_ticks: 0,
            total_ticks: 0,
            last_tick: now,
            last_tick_ms: 0.0,
            writer,
        }
    }

    fn record_sample(&mut self, tps: f64, window_ms: f64) {
        let timestamp_ms = unix_timestamp_ms();

        if let Some(writer) = self.writer.as_mut() {
            if writeln!(
                writer,
                "{},{:.2},{:.2},{:.2},{}",
                timestamp_ms,
                tps,
                window_ms,
                self.last_tick_ms,
                self.total_ticks
            )
            .is_err()
            {
                tracing::warn!("Failed to write TPS sample; disabling TPS file output");
                self.writer = None;
                return;
            }

            let _ = writer.flush();
        }

        tracing::info!(
            tps = tps,
            window_ms = window_ms,
            last_tick_ms = self.last_tick_ms,
            total_ticks = self.total_ticks,
            "TPS sample"
        );
    }
}

pub fn track_tps(mut metrics: ResMut<TpsMetrics>) {
    let now = Instant::now();
    let tick_delta = now.duration_since(metrics.last_tick);
    metrics.last_tick = now;
    metrics.last_tick_ms = tick_delta.as_secs_f64() * 1000.0;

    metrics.total_ticks += 1;
    metrics.window_ticks += 1;

    let elapsed = now.duration_since(metrics.window_start);
    if elapsed >= metrics.report_interval {
        let elapsed_secs = elapsed.as_secs_f64();
        let tps = if elapsed_secs > 0.0 {
            metrics.window_ticks as f64 / elapsed_secs
        } else {
            0.0
        };
        let window_ms = elapsed_secs * 1000.0;

        metrics.record_sample(tps, window_ms);
        metrics.window_start = now;
        metrics.window_ticks = 0;
    }
}

fn resolve_tps_path(tps_output: Option<String>) -> Option<PathBuf> {
    let provided = tps_output
        .map(|path| path.trim().to_string())
        .filter(|path| !path.is_empty())
        .map(PathBuf::from);

    provided.or_else(|| Some(default_tps_path()))
}

fn default_tps_path() -> PathBuf {
    let timestamp = unix_timestamp_ms();
    PathBuf::from(format!("logs/tps-{timestamp}.csv"))
}

fn open_tps_writer(path: &Path) -> Option<BufWriter<File>> {
    if let Some(parent) = path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            tracing::warn!(error = ?err, path = %path.display(), "Failed to create TPS output directory");
            return None;
        }
    }

    match File::create(path) {
        Ok(file) => {
            let mut writer = BufWriter::new(file);
            if writeln!(writer, "timestamp_ms,tps,window_ms,last_tick_ms,total_ticks").is_err() {
                tracing::warn!(path = %path.display(), "Failed to write TPS header");
                return None;
            }
            Some(writer)
        }
        Err(err) => {
            tracing::warn!(error = ?err, path = %path.display(), "Failed to open TPS output file");
            None
        }
    }
}

fn unix_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
