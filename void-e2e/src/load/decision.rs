use serde::Serialize;

use super::measure::{CheckOutcome, Measurements};
use super::workload::Thresholds;

/// One threshold compared with what the run observed.
#[derive(Clone, Debug, Serialize)]
pub struct Verdict {
    pub threshold: &'static str,
    /// How `observed` is compared with `limit`.
    pub rule: &'static str,
    pub limit: f64,
    /// Absent when the run did not observe it.
    pub observed: Option<f64>,
    pub outcome: VerdictOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerdictOutcome {
    Pass,
    Fail,
    /// The target cannot expose this measurement (a server log, for an
    /// endpoint outside this process). It does not count against the run.
    NotApplicable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Decision {
    #[serde(rename = "PASS")]
    Pass,
    #[serde(rename = "FAIL")]
    Fail,
}

impl std::fmt::Display for Decision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
        })
    }
}

const AT_LEAST: &str = "observed >= limit";
const AT_MOST: &str = "observed <= limit";

/// Compares every threshold with the measurements. A threshold whose value
/// was not observed fails, except where the target cannot expose it: the
/// server log of a server outside this process, and the tick rate of such a
/// server when it sends no world-time updates although the load ran. Those
/// are not applicable. The run passes when no verdict fails.
pub fn decide(thresholds: &Thresholds, measurements: &Measurements) -> (Vec<Verdict>, Decision) {
    let failed_checks = measurements
        .smoke_checks
        .iter()
        .filter(|check| check.outcome != CheckOutcome::Passed)
        .count() as f64;
    let smoke_observed = (!measurements.smoke_checks.is_empty()).then_some(failed_checks);
    let errors = &measurements.errors;
    let count = |n: u64| Some(n as f64);

    let verdicts = vec![
        verdict(
            "max_failed_smoke_checks",
            AT_MOST,
            thresholds.max_failed_smoke_checks as f64,
            smoke_observed,
        ),
        verdict(
            "min_connection_success",
            AT_LEAST,
            thresholds.min_connection_success,
            measurements.connections.success_rate,
        ),
        match measurements.tick_rate.judged() {
            None if measurements.load_ran => {
                not_applicable("min_tick_rate", AT_LEAST, thresholds.min_tick_rate)
            }
            observed => verdict(
                "min_tick_rate",
                AT_LEAST,
                thresholds.min_tick_rate,
                observed,
            ),
        },
        verdict(
            "max_latency_p95_ms",
            AT_MOST,
            thresholds.max_latency_p95_ms,
            measurements.latency.all.p95_ms,
        ),
        verdict(
            "max_protocol_errors",
            AT_MOST,
            thresholds.max_protocol_errors as f64,
            count(errors.protocol_errors),
        ),
        verdict(
            "max_unexpected_disconnects",
            AT_MOST,
            thresholds.max_unexpected_disconnects as f64,
            count(errors.unexpected_disconnects),
        ),
        verdict(
            "max_timeouts",
            AT_MOST,
            thresholds.max_timeouts as f64,
            count(errors.timeouts),
        ),
        verdict(
            "max_other_errors",
            AT_MOST,
            thresholds.max_other_errors as f64,
            count(errors.other),
        ),
        match errors.server_log_failures {
            Some(failures) => verdict(
                "max_server_log_failures",
                AT_MOST,
                thresholds.max_server_log_failures as f64,
                count(failures),
            ),
            None => not_applicable(
                "max_server_log_failures",
                AT_MOST,
                thresholds.max_server_log_failures as f64,
            ),
        },
    ];
    let decision = if verdicts
        .iter()
        .any(|verdict| verdict.outcome == VerdictOutcome::Fail)
    {
        Decision::Fail
    } else {
        Decision::Pass
    };
    (verdicts, decision)
}

fn not_applicable(threshold: &'static str, rule: &'static str, limit: f64) -> Verdict {
    Verdict {
        threshold,
        rule,
        limit,
        observed: None,
        outcome: VerdictOutcome::NotApplicable,
    }
}

fn verdict(
    threshold: &'static str,
    rule: &'static str,
    limit: f64,
    observed: Option<f64>,
) -> Verdict {
    let within = observed.is_some_and(|value| {
        if rule == AT_LEAST {
            value >= limit
        } else {
            value <= limit
        }
    });
    Verdict {
        threshold,
        rule,
        limit,
        observed,
        outcome: if within {
            VerdictOutcome::Pass
        } else {
            VerdictOutcome::Fail
        },
    }
}

/// The names of the thresholds that failed, in report order.
pub fn failed(verdicts: &[Verdict]) -> Vec<&'static str> {
    verdicts
        .iter()
        .filter(|verdict| verdict.outcome == VerdictOutcome::Fail)
        .map(|verdict| verdict.threshold)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load::measure::{Connections, Distribution, Errors, Latency, SmokeCheck, TickRate};
    use crate::load::workload::Workload;

    /// What a healthy smoke run records: every check passed, every client
    /// connected, 20 TPS, sub-tick round trips, no errors.
    fn healthy() -> Measurements {
        let latency: Vec<f64> = (0..40).map(|i| 20.0 + f64::from(i)).collect();
        Measurements {
            smoke_checks: vec![SmokeCheck {
                name: "join",
                outcome: CheckOutcome::Passed,
                detail: String::new(),
            }],
            load_ran: true,
            connections: Connections::new(4, 4),
            latency: Latency {
                all: Distribution::of(&latency),
                chat: Distribution::of(&latency[..20]),
                command: Distribution::of(&latency[20..]),
            },
            tick_rate: TickRate {
                nominal: 20.0,
                server_observed: Some(20.0),
                client_observed_min: Some(19.9),
                client_observed_median: Some(20.0),
                client_samples: 4,
            },
            errors: Errors {
                server_log_failures: Some(0),
                ..Default::default()
            },
            resources: None,
        }
    }

    fn judge(measurements: &Measurements) -> (Decision, Vec<&'static str>) {
        let (verdicts, decision) = decide(&Workload::smoke().thresholds, measurements);
        (decision, failed(&verdicts))
    }

    #[test]
    fn healthy_measurements_pass_every_threshold() {
        let (verdicts, decision) = decide(&Workload::smoke().thresholds, &healthy());
        assert_eq!(decision, Decision::Pass);
        assert_eq!(verdicts.len(), 9);
        assert!(
            verdicts
                .iter()
                .all(|verdict| verdict.outcome == VerdictOutcome::Pass)
        );
    }

    /// Each threshold, violated alone, fails the run and is the one named.
    #[test]
    fn each_violated_threshold_fails_the_run_and_is_named() {
        type Breaker = fn(&mut Measurements);
        let cases: [(&str, Breaker); 9] = [
            ("max_failed_smoke_checks", |m| {
                m.smoke_checks[0].outcome = CheckOutcome::Failed;
            }),
            ("min_connection_success", |m| {
                m.connections = Connections::new(4, 3);
            }),
            ("min_tick_rate", |m| {
                m.tick_rate.server_observed = Some(9.99)
            }),
            ("max_latency_p95_ms", |m| {
                // 2 slow round trips out of 20 put the 95th percentile at 2000.1 ms.
                let mut samples = vec![10.0; 18];
                samples.extend([2_000.1; 2]);
                m.latency.all = Distribution::of(&samples);
            }),
            ("max_protocol_errors", |m| m.errors.protocol_errors = 1),
            ("max_unexpected_disconnects", |m| {
                m.errors.unexpected_disconnects = 1
            }),
            ("max_timeouts", |m| m.errors.timeouts = 1),
            ("max_other_errors", |m| m.errors.other = 1),
            ("max_server_log_failures", |m| {
                m.errors.server_log_failures = Some(1)
            }),
        ];
        for (threshold, mut_break) in cases {
            let mut measurements = healthy();
            mut_break(&mut measurements);
            assert_eq!(
                judge(&measurements),
                (Decision::Fail, vec![threshold]),
                "violating {threshold}"
            );
        }
    }

    #[test]
    fn limits_are_inclusive() {
        let thresholds = Workload::smoke().thresholds;
        let mut measurements = healthy();
        measurements.tick_rate.server_observed = Some(thresholds.min_tick_rate);
        measurements.latency.all = Distribution::of(&[thresholds.max_latency_p95_ms]);
        assert_eq!(judge(&measurements), (Decision::Pass, vec![]));
    }

    #[test]
    fn unobserved_measurements_fail() {
        let mut measurements = healthy();
        measurements.load_ran = false;
        measurements.smoke_checks.clear();
        measurements.connections = Connections::new(0, 0);
        measurements.tick_rate.server_observed = None;
        measurements.tick_rate.client_observed_min = None;
        measurements.latency.all = Distribution::of(&[]);
        assert_eq!(
            judge(&measurements),
            (
                Decision::Fail,
                vec![
                    "max_failed_smoke_checks",
                    "min_connection_success",
                    "min_tick_rate",
                    "max_latency_p95_ms",
                ]
            )
        );
    }

    #[test]
    fn tick_rate_falls_back_to_the_slowest_client_without_a_server_count() {
        let mut measurements = healthy();
        measurements.tick_rate.server_observed = None;
        measurements.tick_rate.client_observed_min = Some(9.0);
        measurements.tick_rate.client_observed_median = Some(20.0);
        assert_eq!(
            judge(&measurements),
            (Decision::Fail, vec!["min_tick_rate"])
        );
    }

    #[test]
    fn an_endpoint_without_world_time_has_no_applicable_tick_rate() {
        let mut measurements = healthy();
        measurements.tick_rate.server_observed = None;
        measurements.tick_rate.client_observed_min = None;
        measurements.tick_rate.client_observed_median = None;
        let (verdicts, decision) = decide(&Workload::smoke().thresholds, &measurements);
        assert_eq!(decision, Decision::Pass);
        let tick_rate = verdicts
            .iter()
            .find(|verdict| verdict.threshold == "min_tick_rate")
            .unwrap();
        assert_eq!(tick_rate.outcome, VerdictOutcome::NotApplicable);
    }

    #[test]
    fn server_logs_out_of_reach_are_not_applicable() {
        let mut measurements = healthy();
        measurements.errors.server_log_failures = None;
        let (verdicts, decision) = decide(&Workload::smoke().thresholds, &measurements);
        assert_eq!(decision, Decision::Pass);
        let logs = verdicts
            .iter()
            .find(|verdict| verdict.threshold == "max_server_log_failures")
            .unwrap();
        assert_eq!(logs.outcome, VerdictOutcome::NotApplicable);
    }
}
