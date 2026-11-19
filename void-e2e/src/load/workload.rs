use serde::Serialize;

/// A named, versioned load scenario together with the thresholds it is
/// judged against.
///
/// A workload is fixed: changing what the clients do, how many there are,
/// how long they run or a threshold is a new `version`, so that two reports
/// with the same `name@version` describe the same scenario.
#[derive(Clone, Debug, Serialize)]
pub struct Workload {
    pub name: &'static str,
    pub version: u32,
    pub description: &'static str,
    /// Load clients, on top of the single client of the smoke checks.
    pub clients: usize,
    /// Clients join one after the other, evenly spread over this window.
    pub ramp_up_ms: u64,
    /// How long every client keeps acting once the ramp-up is over. Tick
    /// rate and resources are measured over this window.
    pub steady_ms: u64,
    /// Each client runs one action cycle per interval: one movement step,
    /// one chat round trip, one command round trip.
    pub action_interval_ms: u64,
    /// Blocks walked per cycle, away from spawn and back.
    pub step_blocks: f64,
    /// How far from spawn a client walks before turning back. Each client
    /// walks its own compass direction, so the clients spread over chunks.
    pub roam_radius_blocks: f64,
    /// The command each client runs, without the slash, and a fragment of
    /// the reply it waits for.
    pub command: &'static str,
    pub command_reply: &'static str,
    /// The tick rate the server is expected to keep; the in-process server
    /// is started at this rate.
    pub nominal_tick_rate: u64,
    pub thresholds: Thresholds,
}

/// Limits a run must stay within to PASS. Every limit is inclusive.
#[derive(Clone, Debug, Serialize)]
pub struct Thresholds {
    /// Share of load clients that complete login, configuration and spawn.
    pub min_connection_success: f64,
    /// Ticks per second over the steady window: measured on the server when
    /// it runs in-process, otherwise from the world-time updates clients
    /// receive (not applicable to a server that sends none).
    pub min_tick_rate: f64,
    /// 95th percentile of every chat and command round trip seen by clients.
    pub max_latency_p95_ms: f64,
    /// Undecodable packets and protocol anomalies.
    pub max_protocol_errors: u64,
    /// Kicks and connections closed by the server during the run.
    pub max_unexpected_disconnects: u64,
    /// Actions whose answer did not arrive within the harness timeout.
    pub max_timeouts: u64,
    pub max_other_errors: u64,
    /// Errors and unrecognised packets logged by the server; only observable
    /// when the server runs in-process.
    pub max_server_log_failures: u64,
    /// Smoke checks that may fail. Always 0: the load run is meaningless on a
    /// server that fails them.
    pub max_failed_smoke_checks: u64,
}

impl Workload {
    /// Every workload the tool knows, by name.
    pub fn all() -> [Workload; 2] {
        [Self::smoke(), Self::standard()]
    }

    pub fn by_name(name: &str) -> Option<Workload> {
        Self::all()
            .into_iter()
            .find(|workload| workload.name == name)
    }

    /// `name@version`, how reports and documentation refer to a workload.
    pub fn id(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }

    /// A few seconds with a handful of clients. The end-to-end suite runs it
    /// on every change, on shared machines, so its thresholds catch a broken
    /// or stalled server without depending on how fast the host is.
    pub fn smoke() -> Workload {
        Workload {
            name: "smoke",
            version: 1,
            description: "Functional smoke checks, then 4 clients that join, walk across \
                          chunk boundaries, chat and run a command for 2 seconds.",
            clients: 4,
            ramp_up_ms: 1_000,
            steady_ms: 2_000,
            action_interval_ms: 500,
            step_blocks: 4.0,
            roam_radius_blocks: 32.0,
            command: "ping",
            command_reply: "Pong",
            nominal_tick_rate: 20,
            thresholds: Thresholds {
                min_connection_success: 1.0,
                min_tick_rate: 10.0,
                max_latency_p95_ms: 2_000.0,
                max_protocol_errors: 0,
                max_unexpected_disconnects: 0,
                max_timeouts: 0,
                max_other_errors: 0,
                max_server_log_failures: 0,
                max_failed_smoke_checks: 0,
            },
        }
    }

    /// The opt-in load profile for a dedicated host or a private staging
    /// endpoint: 20 clients for 30 seconds after a 5-second ramp-up.
    pub fn standard() -> Workload {
        Workload {
            name: "standard",
            version: 1,
            description: "Functional smoke checks, then 20 clients joining over 5 seconds \
                          that walk across chunk boundaries, chat and run a command every \
                          second for 30 seconds.",
            clients: 20,
            ramp_up_ms: 5_000,
            steady_ms: 30_000,
            action_interval_ms: 1_000,
            step_blocks: 4.0,
            roam_radius_blocks: 32.0,
            command: "ping",
            command_reply: "Pong",
            nominal_tick_rate: 20,
            thresholds: Thresholds {
                min_connection_success: 1.0,
                min_tick_rate: 18.0,
                max_latency_p95_ms: 250.0,
                max_protocol_errors: 0,
                max_unexpected_disconnects: 0,
                max_timeouts: 0,
                max_other_errors: 0,
                max_server_log_failures: 0,
                max_failed_smoke_checks: 0,
            },
        }
    }

    /// What each client does, in words, for the report.
    pub fn actions(&self) -> Vec<String> {
        vec![
            "join: handshake, login, configuration, spawn in the play state".to_string(),
            format!(
                "every {} ms: walk {} blocks along its own compass direction, out to {} blocks \
                 from spawn and back (crossing chunk boundaries)",
                self.action_interval_ms, self.step_blocks, self.roam_radius_blocks
            ),
            "every cycle: send a chat message and wait for its broadcast to come back".to_string(),
            format!(
                "every cycle: run /{} and wait for a reply containing \"{}\"",
                self.command, self.command_reply
            ),
            "answer keep-alives, pings and teleports like the vanilla client".to_string(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workloads_are_found_by_name_and_have_distinct_ids() {
        assert_eq!(Workload::by_name("smoke").unwrap().id(), "smoke@1");
        assert_eq!(Workload::by_name("standard").unwrap().id(), "standard@1");
        assert!(Workload::by_name("unknown").is_none());
    }
}
