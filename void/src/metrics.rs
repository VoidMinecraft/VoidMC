use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bevy_app::{App, Plugin, PostUpdate};
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::IntoScheduleConfigs;

use crate::config::ServerConfigResource;
use crate::schedule::VoidSystems;

const DEFAULT_REPORT_INTERVAL: Duration = Duration::from_secs(1);
const TPS_CSV_HEADER: &str = "timestamp_ms,tps,window_ms,last_tick_ms,total_ticks";

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
            .add_systems(PostUpdate, track_tps.in_set(VoidSystems::Metrics));
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
    output: Option<TpsOutput>,
}

/// The open TPS CSV file and the path it was opened at, for warnings.
struct TpsOutput {
    path: PathBuf,
    writer: BufWriter<File>,
}

impl TpsMetrics {
    fn new(tps_output: Option<String>) -> Self {
        let now = Instant::now();
        let output = resolve_tps_path(tps_output).and_then(open_tps_output);

        Self {
            report_interval: DEFAULT_REPORT_INTERVAL,
            window_start: now,
            window_ticks: 0,
            total_ticks: 0,
            last_tick: now,
            last_tick_ms: 0.0,
            output,
        }
    }

    fn record_sample(&mut self, tps: f64, window_ms: f64) {
        let timestamp_ms = unix_timestamp_ms();

        if let Some(output) = self.output.as_mut() {
            let written = writeln!(
                output.writer,
                "{},{:.2},{:.2},{:.2},{}",
                timestamp_ms, tps, window_ms, self.last_tick_ms, self.total_ticks
            )
            .and_then(|()| output.writer.flush());
            if let Err(err) = written {
                tracing::warn!(
                    error = %err,
                    path = %output.path.display(),
                    "Failed to write TPS sample; disabling TPS file output"
                );
                self.output = None;
            }
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

pub fn track_tps(mut metrics: ResMut<TpsMetrics>, config: Res<ServerConfigResource>) {
    let now = Instant::now();
    let tick_delta = now.duration_since(metrics.last_tick);
    metrics.last_tick = now;
    metrics.last_tick_ms = tick_delta.as_secs_f64() * 1000.0;

    if config.slow_tick_ms > 0 && metrics.last_tick_ms > config.slow_tick_ms as f64 {
        tracing::warn!(
            last_tick_ms = metrics.last_tick_ms,
            slow_tick_ms = config.slow_tick_ms,
            "Slow tick detected"
        );
    }

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

/// Creates (or replaces) the CSV file and writes its header. Any failure is a
/// warning naming the path and the reason: TPS samples are then only logged.
fn open_tps_output(path: PathBuf) -> Option<TpsOutput> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        && let Err(err) = fs::create_dir_all(parent)
    {
        tracing::warn!(error = %err, path = %path.display(), "Failed to create TPS output directory; TPS samples are only logged");
        return None;
    }

    let file = match File::create(&path) {
        Ok(file) => file,
        Err(err) => {
            tracing::warn!(error = %err, path = %path.display(), "Failed to open TPS output file; TPS samples are only logged");
            return None;
        }
    };

    let mut writer = BufWriter::new(file);
    if let Err(err) = writeln!(writer, "{TPS_CSV_HEADER}").and_then(|()| writer.flush()) {
        tracing::warn!(error = %err, path = %path.display(), "Failed to write TPS header; TPS samples are only logged");
        return None;
    }

    tracing::info!(path = %path.display(), "Writing TPS samples to {}", path.display());
    Some(TpsOutput { path, writer })
}

fn unix_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use bevy_app::App;

    use super::*;
    use crate::config::ServerConfig;

    /// A directory under the system temp dir, removed when dropped (also when
    /// the test panics).
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!(
                "voidmc-metrics-{name}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self, relative: &str) -> PathBuf {
            self.0.join(relative)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Every event logged while `run` runs: its level, then its fields
    /// (message included) as ` name=value` pairs.
    #[derive(Clone, Default)]
    struct Logs(Arc<Mutex<Vec<(tracing::Level, String)>>>);

    impl Logs {
        fn capture<R>(run: impl FnOnce() -> R) -> (R, Self) {
            let logs = Self::default();
            let result = tracing::subscriber::with_default(logs.clone(), run);
            (result, logs)
        }

        fn at(&self, level: tracing::Level) -> Vec<String> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .filter(|(l, _)| *l == level)
                .map(|(_, line)| line.clone())
                .collect()
        }

        fn all(&self) -> Vec<String> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .map(|(_, line)| line.clone())
                .collect()
        }
    }

    struct Line(String);

    impl tracing::field::Visit for Line {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            self.0.push_str(&format!(" {}={value:?}", field.name()));
        }
    }

    impl tracing::Subscriber for Logs {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }

        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

        fn event(&self, event: &tracing::Event<'_>) {
            let mut line = Line(String::new());
            event.record(&mut line);
            self.0
                .lock()
                .unwrap()
                .push((*event.metadata().level(), line.0));
        }

        fn enter(&self, _: &tracing::span::Id) {}

        fn exit(&self, _: &tracing::span::Id) {}
    }

    fn assert_no_success_claim(logs: &Logs) {
        let claims: Vec<String> = logs
            .all()
            .into_iter()
            .filter(|line| line.contains("Writing TPS samples"))
            .collect();
        assert!(
            claims.is_empty(),
            "claims a TPS file is written: {claims:?}"
        );
    }

    /// Checks that exactly one warning was logged and that it names `path`
    /// and gives a reason.
    fn assert_one_warning_naming(logs: &Logs, path: &Path) {
        let warnings = logs.at(tracing::Level::WARN);
        assert_eq!(warnings.len(), 1, "expected one warning: {warnings:?}");
        let warning = &warnings[0];
        assert!(
            warning.contains(&format!("path={}", path.display())),
            "the warning names the path: {warning}"
        );
        assert!(
            warning.contains("error="),
            "the warning gives the reason: {warning}"
        );
    }

    #[test]
    fn default_path_is_a_timestamped_csv_under_logs() {
        for unset in [None, Some(String::new()), Some("   ".to_string())] {
            let path = resolve_tps_path(unset).unwrap();
            let name = path.to_str().unwrap();
            let stamp = name
                .strip_prefix("logs/tps-")
                .and_then(|rest| rest.strip_suffix(".csv"))
                .unwrap_or_else(|| panic!("not logs/tps-<timestamp>.csv: {name}"));
            assert!(stamp.parse::<u128>().is_ok(), "not a timestamp: {name}");
        }
        assert_eq!(
            resolve_tps_path(Some(" out/tps.csv ".to_string())),
            Some(PathBuf::from("out/tps.csv"))
        );
    }

    #[test]
    fn creates_missing_directories_and_writes_the_header_at_once() {
        let dir = TempDir::new("missing-parent");
        let path = dir.path("a/b/tps.csv");

        let (output, logs) = Logs::capture(|| open_tps_output(path.clone()));

        let output = output.expect("the file opens");
        // Flushed before any sample: a server stopped within its first
        // second still leaves a valid CSV.
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("{TPS_CSV_HEADER}\n")
        );
        assert!(logs.at(tracing::Level::WARN).is_empty());
        let claim = logs
            .at(tracing::Level::INFO)
            .into_iter()
            .find(|line| line.contains("Writing TPS samples"))
            .expect("says where the samples go");
        assert!(claim.contains(&path.display().to_string()), "{claim}");
        drop(output);
    }

    #[test]
    fn an_existing_file_is_replaced_not_appended_to() {
        let dir = TempDir::new("replace");
        let path = dir.path("tps.csv");
        fs::write(
            &path,
            format!("{TPS_CSV_HEADER}\n1,20.00,1000.00,50.00,20\nstale\n"),
        )
        .unwrap();

        let mut metrics = TpsMetrics::new(Some(path.display().to_string()));
        metrics.total_ticks = 7;
        metrics.record_sample(19.5, 1001.0);
        drop(metrics);

        let content = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2, "{content:?}");
        assert_eq!(lines[0], TPS_CSV_HEADER);
        assert!(lines[1].ends_with(",19.50,1001.00,0.00,7"), "{content:?}");
    }

    #[test]
    fn a_directory_as_output_path_warns_and_writes_nothing() {
        let dir = TempDir::new("is-dir");
        let path = dir.path("taken");
        fs::create_dir(&path).unwrap();

        let (output, logs) = Logs::capture(|| open_tps_output(path.clone()));

        assert!(output.is_none());
        assert_one_warning_naming(&logs, &path);
        assert_no_success_claim(&logs);
        assert!(path.is_dir());
    }

    #[test]
    fn a_path_under_a_regular_file_warns_and_writes_nothing() {
        let dir = TempDir::new("under-file");
        fs::write(dir.path("file"), "not a directory").unwrap();
        let path = dir.path("file/tps.csv");

        let (output, logs) = Logs::capture(|| open_tps_output(path.clone()));

        assert!(output.is_none());
        assert_one_warning_naming(&logs, &path);
        assert_no_success_claim(&logs);
        assert_eq!(
            fs::read_to_string(dir.path("file")).unwrap(),
            "not a directory"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_header_that_cannot_be_written_warns() {
        let path = PathBuf::from("/dev/full");

        let (output, logs) = Logs::capture(|| open_tps_output(path.clone()));

        assert!(output.is_none());
        assert_one_warning_naming(&logs, &path);
        assert_no_success_claim(&logs);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_sample_that_cannot_be_written_warns_once_and_stops_file_output() {
        let path = PathBuf::from("/dev/full");
        let mut metrics = TpsMetrics::new(Some("/dev/null".to_string()));
        metrics.output = Some(TpsOutput {
            path: path.clone(),
            writer: BufWriter::new(File::create(&path).unwrap()),
        });

        let ((), logs) = Logs::capture(|| {
            metrics.record_sample(20.0, 1000.0);
            metrics.record_sample(20.0, 1000.0);
        });

        assert_one_warning_naming(&logs, &path);
        assert!(metrics.output.is_none());
        // Sampling itself goes on: both samples are still logged.
        let samples = logs
            .at(tracing::Level::INFO)
            .into_iter()
            .filter(|line| line.contains("TPS sample"))
            .count();
        assert_eq!(samples, 2);
    }

    #[test]
    fn each_report_window_appends_one_parsable_row() {
        let dir = TempDir::new("rows");
        let path = dir.path("tps.csv");
        let mut app = App::new();
        app.insert_resource(ServerConfigResource::from(&ServerConfig::default()))
            .add_plugins(MetricsPlugin::new(Some(path.display().to_string())));
        // Close a window on every tick instead of every second.
        app.world_mut().resource_mut::<TpsMetrics>().report_interval = Duration::ZERO;

        for _ in 0..3 {
            app.update();
        }

        let content = fs::read_to_string(&path).unwrap();
        let mut lines = content.lines();
        assert_eq!(lines.next(), Some(TPS_CSV_HEADER));
        let rows: Vec<Vec<&str>> = lines.map(|line| line.split(',').collect()).collect();
        assert_eq!(rows.len(), 3, "{content:?}");
        for (index, row) in rows.iter().enumerate() {
            assert_eq!(row.len(), 5, "{row:?}");
            row[0].parse::<u128>().unwrap();
            for column in &row[1..4] {
                assert!(column.parse::<f64>().unwrap() >= 0.0, "{row:?}");
            }
            assert_eq!(row[4].parse::<u64>().unwrap(), index as u64 + 1);
        }
    }
}
