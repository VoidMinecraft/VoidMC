use serde::Serialize;

/// One functional check run by a single client before the load.
#[derive(Clone, Debug, Serialize)]
pub struct SmokeCheck {
    pub name: &'static str,
    pub outcome: CheckOutcome,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckOutcome {
    Passed,
    Failed,
    /// Not run, because an earlier check failed.
    Skipped,
}

/// Everything a run observed. `None` means "not observed": the run did not
/// get far enough, or the target does not expose it.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Measurements {
    pub smoke_checks: Vec<SmokeCheck>,
    /// False when the load phase was skipped because a smoke check failed.
    pub load_ran: bool,
    pub connections: Connections,
    pub latency: Latency,
    pub tick_rate: TickRate,
    pub errors: Errors,
    pub resources: Option<Resources>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Connections {
    pub attempted: u64,
    pub succeeded: u64,
    /// `succeeded / attempted`, absent when nothing was attempted.
    pub success_rate: Option<f64>,
}

impl Connections {
    pub fn new(attempted: u64, succeeded: u64) -> Self {
        Self {
            attempted,
            succeeded,
            success_rate: (attempted > 0).then(|| succeeded as f64 / attempted as f64),
        }
    }
}

/// Round trips as clients see them: from sending a chat message or command
/// to receiving the server's answer. Includes the wait for the next tick.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Latency {
    pub all: Distribution,
    pub chat: Distribution,
    pub command: Distribution,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Distribution {
    pub samples: u64,
    pub p50_ms: Option<f64>,
    pub p95_ms: Option<f64>,
    pub max_ms: Option<f64>,
}

impl Distribution {
    /// Nearest-rank percentiles of `samples`, in milliseconds.
    pub fn of(samples: &[f64]) -> Self {
        let mut sorted = samples.to_vec();
        sorted.sort_by(f64::total_cmp);
        let rank = |percent: f64| {
            let index = ((percent / 100.0) * sorted.len() as f64).ceil() as usize;
            sorted.get(index.saturating_sub(1)).copied()
        };
        Self {
            samples: sorted.len() as u64,
            p50_ms: rank(50.0),
            p95_ms: rank(95.0),
            max_ms: sorted.last().copied(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct TickRate {
    pub nominal: f64,
    /// Game ticks per second over the steady window, read from the server's
    /// own game time. Only when the server runs in-process.
    pub server_observed: Option<f64>,
    /// Game-time advance per second between the first and the last
    /// world-time update each client received.
    pub client_observed_min: Option<f64>,
    pub client_observed_median: Option<f64>,
    pub client_samples: u64,
}

impl TickRate {
    /// The value judged against `min_tick_rate`: the server's own count when
    /// available, otherwise the slowest client's view.
    pub fn judged(&self) -> Option<f64> {
        self.server_observed.or(self.client_observed_min)
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Errors {
    pub connection_failures: u64,
    pub protocol_errors: u64,
    pub unexpected_disconnects: u64,
    pub timeouts: u64,
    pub other: u64,
    /// Errors and unrecognised packets in the server log. Absent when the
    /// server is not in this process.
    pub server_log_failures: Option<u64>,
    /// The first error messages, for diagnosis.
    pub details: Vec<String>,
}

/// Resource use over the steady window, when the server runs in this process.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Resources {
    pub window_ms: f64,
    /// CPU time of the server's game (ECS) thread.
    pub game_thread_cpu_ms: Option<f64>,
    /// `game_thread_cpu_ms / window_ms`: 1.0 is one core fully busy.
    pub game_thread_utilisation: Option<f64>,
    /// CPU time of every server thread: game thread, Bevy task pools (where
    /// most systems run) and the network runtime.
    pub server_threads_cpu_ms: Option<f64>,
    /// `server_threads_cpu_ms / window_ms`.
    pub server_threads_utilisation: Option<f64>,
    /// CPU time of the whole process: server and load clients together.
    pub process_cpu_ms: Option<f64>,
    /// Highest resident set size sampled during the window, whole process.
    pub process_rss_peak_kib: Option<u64>,
    pub scope: &'static str,
}

/// The category of a failure seen by a client, from the harness message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Protocol,
    Disconnect,
    Timeout,
    Other,
}

impl ErrorKind {
    pub fn classify(message: &str) -> Self {
        if message.contains("undecodable packet") || message.contains("protocol anomal") {
            Self::Protocol
        } else if message.contains("kicked:")
            || message.contains("closed the connection")
            || message.contains("could not send a packet")
        {
            Self::Disconnect
        } else if message.contains("timed out") {
            Self::Timeout
        } else {
            Self::Other
        }
    }
}

impl Errors {
    const KEPT_DETAILS: usize = 20;

    pub fn record(&mut self, kind: ErrorKind, who: &str, message: &str) {
        match kind {
            ErrorKind::Protocol => self.protocol_errors += 1,
            ErrorKind::Disconnect => self.unexpected_disconnects += 1,
            ErrorKind::Timeout => self.timeouts += 1,
            ErrorKind::Other => self.other += 1,
        }
        self.detail(who, message);
    }

    pub fn connection_failure(&mut self, who: &str, message: &str) {
        self.connection_failures += 1;
        self.detail(who, message);
    }

    pub fn detail(&mut self, who: &str, message: &str) {
        if self.details.len() < Self::KEPT_DETAILS {
            let line = message.lines().next().unwrap_or_default();
            self.details.push(format!("{who}: {line}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_use_the_nearest_rank() {
        let samples: Vec<f64> = (1..=20).map(f64::from).collect();
        let distribution = Distribution::of(&samples);
        assert_eq!(distribution.samples, 20);
        assert_eq!(distribution.p50_ms, Some(10.0));
        assert_eq!(distribution.p95_ms, Some(19.0));
        assert_eq!(distribution.max_ms, Some(20.0));
        assert_eq!(Distribution::of(&[7.0]).p95_ms, Some(7.0));
        assert_eq!(Distribution::of(&[]).p95_ms, None);
    }

    #[test]
    fn harness_messages_are_classified() {
        let cases = [
            (
                "waiting for x: undecodable packet: leftover bytes",
                ErrorKind::Protocol,
            ),
            ("waiting for x: kicked: bye", ErrorKind::Disconnect),
            (
                "waiting for x: the server closed the connection",
                ErrorKind::Disconnect,
            ),
            ("timed out waiting for x", ErrorKind::Timeout),
            ("something else", ErrorKind::Other),
        ];
        for (message, kind) in cases {
            assert_eq!(ErrorKind::classify(message), kind, "{message}");
        }
    }

    #[test]
    fn connection_rate_is_absent_without_attempts() {
        assert_eq!(Connections::new(0, 0).success_rate, None);
        assert_eq!(Connections::new(4, 3).success_rate, Some(0.75));
    }
}
