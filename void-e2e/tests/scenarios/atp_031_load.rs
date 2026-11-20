//! VMC-ATP-031: functional smoke checks and a multi-client load run with
//! full-protocol clients, judged against a workload's thresholds.

use std::path::PathBuf;
use std::time::Duration;

use bevy_app::Update;
use bevy_ecs::entity::Entity;
use voidmc::components::PlayerName;
use voidmc::events::{ChatMessageEvent, PlayerJoinEvent};
use voidmc::{On, Players, Query, Text, TextColor, WorldTime};
use voidmc_e2e::TestServer;
use voidmc_e2e::load::{
    self, CheckOutcome, Decision, REPORT_FORMAT, Report, SCOPE, Target, VerdictOutcome, Workload,
};
use voidmc_protocol::clientbound::Disconnect;

/// A directory under the system temp dir, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("voidmc-load-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn kick(players: &Players, entity: Entity) {
    players.send(
        entity,
        Disconnect {
            reason: Text::truncated("load test kick").component(TextColor::Red),
        },
    );
}

/// The first load client of a run, `load-<run>-0`.
fn first_load_client(name: &str) -> bool {
    name.starts_with("load-") && name.ends_with("-0")
}

fn failed(report: &Report) -> Vec<&'static str> {
    report.failed_thresholds.clone()
}

#[tokio::test]
async fn smoke_workload_passes_in_process_and_reports_every_clause() {
    let workload = Workload::smoke();
    let report = load::run(&workload, Target::InProcess).await.unwrap();
    let summary = report.summary();
    assert_eq!(report.decision, Decision::Pass, "{summary}");
    assert!(report.failed_thresholds.is_empty());

    // The workload is identified.
    assert_eq!(report.workload.id, "smoke@1");
    assert_eq!(report.workload.definition.clients, 4);
    assert!(!report.workload.actions.is_empty());

    // The run conditions are recorded.
    let conditions = &report.conditions;
    assert_eq!(conditions.endpoint_kind, "in-process");
    assert!(conditions.endpoint.starts_with("127.0.0.1:"));
    assert_ne!(conditions.endpoint, "127.0.0.1:25565");
    assert_eq!(conditions.server.as_ref().unwrap().tick_rate, 20);
    assert!(conditions.host_cores > 0);
    assert_eq!(conditions.build_profile, "dev");
    assert_eq!(conditions.commit.len(), 40, "a git commit hash");
    assert!(conditions.started_at_utc.ends_with('Z'));
    assert!(conditions.wall_clock_ms > 0);

    // Every smoke check ran and passed before the load.
    let m = &report.measurements;
    let checks: Vec<_> = m.smoke_checks.iter().map(|check| check.name).collect();
    assert_eq!(
        checks,
        [
            "status_ping",
            "join",
            "chunks_loaded",
            "chat_echo",
            "command_reply",
            "chunk_boundary_crossing",
            "healthy"
        ]
    );
    assert!(
        m.smoke_checks
            .iter()
            .all(|check| check.outcome == CheckOutcome::Passed)
    );
    assert!(m.load_ran);

    // Connection success, latency, tick rate, errors and resources.
    assert_eq!((m.connections.attempted, m.connections.succeeded), (4, 4));
    assert_eq!(m.connections.success_rate, Some(1.0));
    assert!(m.latency.chat.samples >= 4 && m.latency.command.samples >= 4);
    assert_eq!(
        m.latency.all.samples,
        m.latency.chat.samples + m.latency.command.samples
    );
    assert!(m.latency.all.p95_ms.is_some());
    assert!(m.tick_rate.server_observed.is_some_and(|rate| rate > 0.0));
    assert_eq!(m.tick_rate.client_samples, 4);
    assert_eq!(m.errors.connection_failures, 0);
    assert_eq!(m.errors.server_log_failures, Some(0));
    assert!(m.errors.details.is_empty(), "{:?}", m.errors.details);
    let resources = m.resources.as_ref().expect("in-process resources");
    assert!(resources.window_ms > 0.0);
    if cfg!(target_os = "linux") {
        let game = resources.game_thread_cpu_ms.unwrap();
        let server = resources.server_threads_cpu_ms.unwrap();
        assert!(
            server >= game,
            "the game thread is one of the server threads"
        );
        // The clients' packets are read, framed and written on the server's
        // network runtime, whose threads are server threads too.
        assert!(
            server > game,
            "the server threads' CPU ({server} ms) includes the network runtime's, \
             beyond the game thread's ({game} ms)"
        );
        assert!(resources.process_cpu_ms.is_some());
        assert!(resources.process_rss_peak_kib.is_some_and(|kib| kib > 0));
        assert!(report.unavailable.is_empty(), "{:?}", report.unavailable);
    }

    // Each threshold is compared, and the scope forbids a capacity claim.
    let thresholds: Vec<_> = report.verdicts.iter().map(|v| v.threshold).collect();
    assert_eq!(
        thresholds,
        [
            "max_failed_smoke_checks",
            "min_connection_success",
            "min_tick_rate",
            "max_latency_p95_ms",
            "max_protocol_errors",
            "max_unexpected_disconnects",
            "max_timeouts",
            "max_other_errors",
            "max_server_log_failures"
        ]
    );
    assert!(
        report
            .verdicts
            .iter()
            .all(|verdict| verdict.observed.is_some() && verdict.outcome == VerdictOutcome::Pass)
    );
    assert_eq!(report.scope, SCOPE);
    assert!(SCOPE.contains("not a capacity claim"));
    assert!(summary.contains("**Decision: PASS**"));
    assert!(summary.contains(SCOPE));

    // The machine-readable report round-trips through a file.
    let dir = TempDir::new("pass");
    let path = dir.0.join("reports/smoke.json");
    report.write(&path).unwrap();
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(json["report_format"], REPORT_FORMAT);
    assert_eq!(json["decision"], "PASS");
    assert_eq!(json["workload"]["id"], "smoke@1");
    assert_eq!(json["workload"]["thresholds"]["min_tick_rate"], 10.0);
    assert_eq!(json["conditions"]["endpoint_kind"], "in-process");
    assert_eq!(json["measurements"]["connections"]["succeeded"], 4);
    assert_eq!(json["verdicts"].as_array().unwrap().len(), 9);
    assert_eq!(json["scope"], SCOPE);
    let markdown = std::fs::read_to_string(path.with_extension("md")).unwrap();
    assert_eq!(markdown, summary);
}

#[tokio::test]
async fn an_operator_endpoint_is_judged_on_client_measurements_only() {
    let server = TestServer::builder()
        .config(|config| config.tick_rate(20))
        .start()
        .await;
    server
        .with_world(|world| {
            world.spawn(WorldTime::new());
        })
        .await;
    let endpoint = server.address().to_string();

    let report = load::run(&Workload::smoke(), Target::Endpoint(endpoint.clone()))
        .await
        .unwrap();
    assert_eq!(report.decision, Decision::Pass, "{}", report.summary());
    assert_eq!(report.conditions.endpoint_kind, "operator-endpoint");
    assert_eq!(report.conditions.endpoint, endpoint);
    assert!(report.conditions.server.is_none());

    let m = &report.measurements;
    assert_eq!(m.tick_rate.server_observed, None);
    assert_eq!(m.tick_rate.client_samples, 4);
    let tick_rate = report
        .verdicts
        .iter()
        .find(|verdict| verdict.threshold == "min_tick_rate")
        .unwrap();
    assert_eq!(tick_rate.observed, m.tick_rate.client_observed_min);
    assert_eq!(tick_rate.outcome, VerdictOutcome::Pass);
    assert!(m.resources.is_none());
    assert_eq!(m.errors.server_log_failures, None);
    let logs = report
        .verdicts
        .iter()
        .find(|verdict| verdict.threshold == "max_server_log_failures")
        .unwrap();
    assert_eq!(logs.outcome, VerdictOutcome::NotApplicable);
    for missing in [
        "tick_rate.server_observed",
        "resources",
        "errors.server_log_failures",
    ] {
        assert!(
            report
                .unavailable
                .iter()
                .any(|line| line.starts_with(missing)),
            "{missing} is listed as unavailable: {:?}",
            report.unavailable
        );
    }
    assert!(report.summary().contains("## Not measured"));

    server.stop().await;
}

#[tokio::test]
async fn an_unreachable_endpoint_fails_the_smoke_checks_and_skips_the_load() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    drop(listener);

    let report = load::run(&Workload::smoke(), Target::Endpoint(endpoint))
        .await
        .unwrap();
    assert_eq!(report.decision, Decision::Fail);
    let m = &report.measurements;
    assert_eq!(m.smoke_checks[0].name, "status_ping");
    assert_eq!(m.smoke_checks[0].outcome, CheckOutcome::Failed);
    assert!(
        m.smoke_checks[1..]
            .iter()
            .all(|check| check.outcome == CheckOutcome::Skipped)
    );
    assert!(!m.load_ran);
    assert_eq!(
        failed(&report),
        [
            "max_failed_smoke_checks",
            "min_connection_success",
            "min_tick_rate",
            "max_latency_p95_ms"
        ]
    );
    assert!(
        report
            .unavailable
            .iter()
            .any(|line| line.contains("skipped"))
    );
    assert!(report.summary().contains("**Decision: FAIL**"));
}

#[tokio::test]
async fn an_unknown_endpoint_is_refused_before_running() {
    let error = load::run(
        &Workload::smoke(),
        Target::Endpoint("no-port-given".to_string()),
    )
    .await
    .unwrap_err();
    assert!(error.contains("cannot resolve"), "{error}");
}

#[tokio::test]
async fn a_client_kicked_on_join_fails_the_connection_success_threshold() {
    let builder = TestServer::builder().plugin(|app| {
        app.add_observer(
            |join: On<PlayerJoinEvent>, players: Players, names: Query<&PlayerName>| {
                if names
                    .get(join.entity)
                    .is_ok_and(|name| first_load_client(&name.0))
                {
                    kick(&players, join.entity);
                }
            },
        );
    });
    let report = load::run_in_process(&Workload::smoke(), builder).await;

    let m = &report.measurements;
    assert_eq!((m.connections.attempted, m.connections.succeeded), (4, 3));
    assert_eq!(m.errors.connection_failures, 1);
    assert!(
        m.errors.details.iter().any(|line| line.contains("kicked")),
        "{:?}",
        m.errors.details
    );
    assert_eq!(report.decision, Decision::Fail);
    assert_eq!(failed(&report), ["min_connection_success"]);
}

#[tokio::test]
async fn a_client_kicked_mid_run_fails_the_unexpected_disconnects_threshold() {
    let builder = TestServer::builder().plugin(|app| {
        app.add_observer(
            |chat: On<ChatMessageEvent>, players: Players, names: Query<&PlayerName>| {
                if names
                    .get(chat.entity)
                    .is_ok_and(|name| first_load_client(&name.0))
                {
                    kick(&players, chat.entity);
                }
            },
        );
    });
    let report = load::run_in_process(&Workload::smoke(), builder).await;

    let m = &report.measurements;
    assert_eq!(m.connections.succeeded, 4);
    assert_eq!(m.errors.unexpected_disconnects, 1);
    assert_eq!(report.decision, Decision::Fail);
    assert_eq!(failed(&report), ["max_unexpected_disconnects"]);
}

#[tokio::test]
async fn a_server_error_log_fails_the_server_log_threshold() {
    let builder = TestServer::builder().plugin(|app| {
        app.add_observer(|chat: On<ChatMessageEvent>, names: Query<&PlayerName>| {
            if names
                .get(chat.entity)
                .is_ok_and(|name| first_load_client(&name.0))
            {
                tracing::error!(target: "voidmc::load_test", "induced server error");
            }
        });
    });
    let report = load::run_in_process(&Workload::smoke(), builder).await;

    let m = &report.measurements;
    assert!(m.errors.server_log_failures.is_some_and(|n| n >= 1));
    assert!(
        m.errors
            .details
            .iter()
            .any(|line| line.starts_with("server log") && line.contains("induced server error")),
        "{:?}",
        m.errors.details
    );
    assert_eq!(report.decision, Decision::Fail);
    assert_eq!(failed(&report), ["max_server_log_failures"]);
}

#[tokio::test]
async fn a_server_below_the_tick_rate_fails_the_tick_rate_threshold() {
    // Every tick takes at least 120 ms: at most 8.3 ticks per second, under
    // the smoke workload's 10.
    let builder = TestServer::builder().plugin(|app| {
        app.add_systems(Update, || std::thread::sleep(Duration::from_millis(120)));
    });
    let report = load::run_in_process(&Workload::smoke(), builder).await;

    let observed = report.measurements.tick_rate.server_observed.unwrap();
    assert!(observed < 8.4, "{observed} ticks per second");
    assert_eq!(report.decision, Decision::Fail);
    assert_eq!(failed(&report), ["min_tick_rate"]);
}
