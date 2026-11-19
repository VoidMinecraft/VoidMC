use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use super::decision::{Decision, Verdict, VerdictOutcome, decide, failed};
use super::measure::{CheckOutcome, Measurements};
use super::workload::{Thresholds, Workload};

/// Identifies the shape of the JSON report; bumped on incompatible changes.
pub const REPORT_FORMAT: &str = "voidmc.load.report.v1";

/// The statement every report carries, so that no number in it is read as a
/// capacity claim.
pub const SCOPE: &str = "These results apply only to the workload, thresholds, host and run \
conditions recorded in this report. They are not a capacity claim: they do not say how many \
players the server supports, nor how it behaves under any other workload or on any other host.";

/// The outcome of one run: what ran, where, what was observed and whether
/// it passed.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub report_format: &'static str,
    pub workload: WorkloadSection,
    pub conditions: Conditions,
    pub measurements: Measurements,
    /// Measurements this run could not make, and why.
    pub unavailable: Vec<String>,
    pub verdicts: Vec<Verdict>,
    pub decision: Decision,
    pub failed_thresholds: Vec<&'static str>,
    pub scope: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkloadSection {
    pub id: String,
    #[serde(flatten)]
    pub definition: Workload,
    pub actions: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Conditions {
    pub started_at_utc: String,
    pub wall_clock_ms: u64,
    /// The checked-out commit of the source tree the tool runs from.
    pub commit: String,
    /// Whether that tree had uncommitted changes.
    pub tree_dirty: Option<bool>,
    pub build_profile: &'static str,
    pub debug_assertions: bool,
    pub host_cores: usize,
    pub os: &'static str,
    pub arch: &'static str,
    /// `in-process` (a server started by the tool on an ephemeral port) or
    /// `operator-endpoint` (an address the operator supplied).
    pub endpoint_kind: &'static str,
    pub endpoint: String,
    /// The configuration of the in-process server; absent for an endpoint.
    pub server: Option<ServerSettings>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ServerSettings {
    pub tick_rate: u64,
    pub view_distance: i32,
    pub simulation_distance: i32,
    pub max_players: i32,
}

impl Conditions {
    pub(crate) fn capture(
        endpoint_kind: &'static str,
        endpoint: String,
        server: Option<ServerSettings>,
    ) -> Self {
        let (commit, tree_dirty) = git_state();
        Self {
            started_at_utc: utc_now(),
            wall_clock_ms: 0,
            commit,
            tree_dirty,
            build_profile: if cfg!(debug_assertions) {
                "dev"
            } else {
                "release"
            },
            debug_assertions: cfg!(debug_assertions),
            host_cores: std::thread::available_parallelism().map_or(0, usize::from),
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            endpoint_kind,
            endpoint,
            server,
        }
    }
}

impl Report {
    pub(crate) fn new(
        workload: &Workload,
        conditions: Conditions,
        measurements: Measurements,
        unavailable: Vec<String>,
    ) -> Self {
        let (verdicts, decision) = decide(&workload.thresholds, &measurements);
        Self {
            report_format: REPORT_FORMAT,
            workload: WorkloadSection {
                id: workload.id(),
                definition: workload.clone(),
                actions: workload.actions(),
            },
            conditions,
            measurements,
            unavailable,
            failed_thresholds: failed(&verdicts),
            verdicts,
            decision,
            scope: SCOPE,
        }
    }

    pub fn passed(&self) -> bool {
        self.decision == Decision::Pass
    }

    pub fn thresholds(&self) -> &Thresholds {
        &self.workload.definition.thresholds
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a report always serialises")
    }

    /// Writes the JSON report to `path` and the summary next to it, with the
    /// `.md` extension.
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.to_json() + "\n")?;
        std::fs::write(path.with_extension("md"), self.summary())
    }

    /// A short human-readable summary in Markdown.
    pub fn summary(&self) -> String {
        let c = &self.conditions;
        let m = &self.measurements;
        let mut out = format!(
            "# VoidMC load run: {} ({})\n\n**Decision: {}**",
            self.workload.id, c.endpoint_kind, self.decision
        );
        if !self.failed_thresholds.is_empty() {
            out += &format!(" (failed: {})", self.failed_thresholds.join(", "));
        }
        out += &format!(
            "\n\n{}\n\n## Run conditions\n\n\
             - started {} UTC, {} ms wall clock\n\
             - commit {}{}\n\
             - {} build (debug assertions {}), {} cores, {}/{}\n\
             - endpoint: {} ({})\n",
            self.workload.definition.description,
            c.started_at_utc,
            c.wall_clock_ms,
            c.commit,
            match c.tree_dirty {
                Some(true) => " with uncommitted changes",
                _ => "",
            },
            c.build_profile,
            if c.debug_assertions { "on" } else { "off" },
            c.host_cores,
            c.os,
            c.arch,
            c.endpoint,
            c.endpoint_kind,
        );
        if let Some(server) = &c.server {
            out += &format!(
                "- in-process server: {} TPS, view distance {}, simulation distance {}\n",
                server.tick_rate, server.view_distance, server.simulation_distance
            );
        }

        out += "\n## Smoke checks\n\n";
        for check in &m.smoke_checks {
            let mark = match check.outcome {
                CheckOutcome::Passed => "pass",
                CheckOutcome::Failed => "FAIL",
                CheckOutcome::Skipped => "skipped",
            };
            out += &format!("- {} [{}] {}\n", check.name, mark, check.detail);
        }

        out += &format!(
            "\n## Measurements\n\n\
             - connections: {}/{} load clients joined{}\n\
             - round trips: {} samples, p50 {}, p95 {}, max {}\n\
             - tick rate: nominal {}, server {}, slowest client {}, median client {}\n\
             - errors: {} connection, {} protocol, {} disconnects, {} timeouts, {} other, \
             server log {}\n",
            m.connections.succeeded,
            m.connections.attempted,
            if m.load_ran {
                ""
            } else {
                " (load skipped: a smoke check failed)"
            },
            m.latency.all.samples,
            ms(m.latency.all.p50_ms),
            ms(m.latency.all.p95_ms),
            ms(m.latency.all.max_ms),
            m.tick_rate.nominal,
            num(m.tick_rate.server_observed),
            num(m.tick_rate.client_observed_min),
            num(m.tick_rate.client_observed_median),
            m.errors.connection_failures,
            m.errors.protocol_errors,
            m.errors.unexpected_disconnects,
            m.errors.timeouts,
            m.errors.other,
            m.errors
                .server_log_failures
                .map_or("n/a".to_string(), |n| n.to_string()),
        );
        if let Some(resources) = &m.resources {
            out += &format!(
                "- resources over {:.0} ms: server threads CPU {} ({} of a core), game thread \
                 CPU {} ({}), process CPU {}, process RSS peak {} ({})\n",
                resources.window_ms,
                ms(resources.server_threads_cpu_ms),
                percent(resources.server_threads_utilisation),
                ms(resources.game_thread_cpu_ms),
                percent(resources.game_thread_utilisation),
                ms(resources.process_cpu_ms),
                resources
                    .process_rss_peak_kib
                    .map_or("n/a".to_string(), |kib| format!("{kib} KiB")),
                resources.scope,
            );
        }
        for detail in &m.errors.details {
            out += &format!("- error: {detail}\n");
        }
        if !self.unavailable.is_empty() {
            out += "\n## Not measured\n\n";
            for line in &self.unavailable {
                out += &format!("- {line}\n");
            }
        }

        out += "\n## Thresholds\n\n| Threshold | Rule | Limit | Observed | Outcome |\n|---|---|---|---|---|\n";
        for verdict in &self.verdicts {
            out += &format!(
                "| {} | {} | {} | {} | {} |\n",
                verdict.threshold,
                verdict.rule,
                verdict.limit,
                num(verdict.observed),
                match verdict.outcome {
                    VerdictOutcome::Pass => "pass",
                    VerdictOutcome::Fail => "FAIL",
                    VerdictOutcome::NotApplicable => "n/a",
                }
            );
        }
        out += &format!("\n{}\n", self.scope);
        out
    }
}

fn ms(value: Option<f64>) -> String {
    value.map_or("n/a".to_string(), |v| format!("{v:.1} ms"))
}

fn percent(value: Option<f64>) -> String {
    value.map_or("n/a".to_string(), |v| format!("{:.1}%", v * 100.0))
}

fn num(value: Option<f64>) -> String {
    value.map_or("n/a".to_string(), |v| format!("{v:.2}"))
}

fn git_state() -> (String, Option<bool>) {
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(["-C", env!("CARGO_MANIFEST_DIR")])
            .args(args)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
    };
    let commit = git(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).map(|s| !s.is_empty());
    (commit, dirty)
}

/// The current time as `YYYY-MM-DDTHH:MM:SSZ`.
fn utc_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    format_utc(seconds)
}

fn format_utc(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    // Howard Hinnant's civil-from-days algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_dates_are_formatted() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(format_utc(1_795_525_199), "2026-11-24T12:59:59Z");
    }
}
