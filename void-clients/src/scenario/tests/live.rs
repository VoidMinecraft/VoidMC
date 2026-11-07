use super::*;

#[test]
fn run_refuses_a_probe_of_another_version() {
    let fake = FakeClient::stale(Level::Render);
    let dir = registry::home();
    fs::write(dir.join("s.scn"), "act map\n").unwrap();
    fake.assert_refused(live_run(&dir, &[]));
}

fn live_run(dir: &Path, more: &[&str]) -> Result<(), String> {
    let mut host = Live(
        |_: &str, _, _: Option<String>| Err("no client starts here".to_string()),
        Limits::TEST,
    );
    let mut args = vec!["--on", NAME];
    args.extend(more);
    command(&words(dir, &args), &mut host)
}

fn state_of_a_game() -> Value {
    json!({"ok": true, "state": "connected", "clock": "real"})
}

/// Every verb is fine and at once, but for the ones named.
fn answering(
    level: Level,
    script: impl Fn(&str) -> Option<Reply> + Send + Sync + 'static,
) -> FakeClient {
    FakeClient::scripted(
        level,
        Box::new(move |verb| {
            script(verb).unwrap_or_else(|| Reply::current(Duration::ZERO, state_of_a_game()))
        }),
    )
}

#[test]
fn a_game_slower_than_the_old_limit_lets_a_line_fail_for_its_own_reason() {
    let fake = answering(Level::Render, |verb| match verb {
        "state" => Some(Reply::current(
            Limits::TEST.open * 7 / 12,
            state_of_a_game(),
        )),
        "wait_until 60000 chunks>=1" => Some(Reply::current(
            Duration::ZERO,
            json!({"ok": false, "error": "java.lang.AssertionError: timed out on chunks>=1"}),
        )),
        _ => None,
    });
    let dir = registry::home();
    fs::write(dir.join("s.scn"), "# top\nwait_until 60000 chunks>=1\n").unwrap();
    assert_eq!(live_run(&dir, &[]), Err(String::new()));
    let report = fs::read_to_string(dir.join("out/report.txt")).unwrap();
    assert!(
        report.contains("FAIL   2") && report.contains("wait_until 60000 chunks>=1"),
        "{report}"
    );
    assert!(report.contains("timed out on chunks>=1"), "{report}");
    assert!(!report.contains("os error"), "{report}");
    assert_eq!(
        fake.heard(),
        [
            "state",
            "state",
            "wait_until 60000 chunks>=1",
            "state",
            "clock real"
        ]
    );
}

#[test]
fn a_game_past_the_limit_of_state_is_said_not_to_answer() {
    let _fake = answering(Level::Render, |verb| {
        (verb == "state").then(|| Reply::current(NEVER_IN_TIME, state_of_a_game()))
    });
    let dir = registry::home();
    fs::write(dir.join("s.scn"), "act map\n").unwrap();
    let error = live_run(&dir, &[]).unwrap_err();
    assert!(
        error.contains("client fake does not answer: no reply to state within 600ms"),
        "{error}"
    );
}

#[test]
fn a_late_answer_never_passes_for_the_answer_to_the_next_verb() {
    let fake = answering(Level::Render, |verb| {
        verb.starts_with("wait_until")
            .then(|| Reply::current(LATE, json!({"ok": false, "error": "the late answer"})))
    });
    let dir = registry::home();
    fs::write(dir.join("s.scn"), "wait_until 1 chunks>=1\nact map\n").unwrap();
    assert_eq!(live_run(&dir, &["--keep-going"]), Err(String::new()));
    let report = fs::read_to_string(dir.join("out/report.txt")).unwrap();
    assert!(
        report.contains("client fake does not answer: no reply to wait_until 1 chunks>=1"),
        "{report}"
    );
    assert!(report.contains("ok     2"), "{report}");
    assert!(!report.contains("the late answer"), "{report}");
    assert_eq!(
        fake.heard(),
        [
            "state",
            "state",
            "wait_until 1 chunks>=1",
            "state",
            "act map",
            "clock real"
        ]
    );
}

/// Past the limit of a `wait_until 1`, and early enough to reach a connection left open.
const LATE: Duration = Duration::from_millis(500);
/// Past every limit of a test, which never waits for it.
const NEVER_IN_TIME: Duration = Duration::from_secs(10);
/// Past the limit of an opening, within the limit of a verb.
const SLOW: Duration = Duration::from_secs(1);

fn garbled() -> Reply {
    Reply {
        after: Duration::ZERO,
        line: "not json".to_string(),
    }
}

#[test]
fn an_answer_that_is_not_json_takes_the_connection_down() {
    let fake = answering(Level::Render, |verb| (verb == "act a").then(garbled));
    let dir = registry::home();
    fs::write(dir.join("s.scn"), "act a\nact b\n").unwrap();
    assert_eq!(live_run(&dir, &["--keep-going"]), Err(String::new()));
    let report = fs::read_to_string(dir.join("out/report.txt")).unwrap();
    assert!(report.contains("FAIL   1"), "{report}");
    assert!(report.contains("ok     2"), "{report}");
    assert_eq!(
        fake.heard(),
        ["state", "state", "act a", "state", "act b", "clock real"]
    );
}

#[test]
fn the_clock_of_the_end_of_a_run_gets_the_limit_of_a_verb_and_not_of_an_opening() {
    let fake = answering(Level::Render, |verb| {
        (verb == "clock real").then(|| Reply::current(SLOW, state_of_a_game()))
    });
    let dir = registry::home();
    fs::write(dir.join("s.scn"), "act map\n").unwrap();
    live_run(&dir, &[]).unwrap();
    assert_eq!(fake.heard(), ["state", "state", "act map", "clock real"]);
}

#[test]
fn the_clock_of_the_end_of_a_run_stops_at_its_limit() {
    let _fake = answering(Level::Render, |verb| {
        (verb == "clock real").then(|| Reply::current(NEVER_IN_TIME, state_of_a_game()))
    });
    let dir = registry::home();
    fs::write(dir.join("s.scn"), "act map\n").unwrap();
    assert_eq!(live_run(&dir, &[]), Err(String::new()));
    let report = fs::read_to_string(dir.join("out/report.txt")).unwrap();
    assert!(report.contains("was not given back"), "{report}");
    assert!(
        report.contains("no reply to clock real within 1.5s"),
        "{report}"
    );
}

/// A wait that loses its connection, a slow `clock real`, and `reply` from the `from`th `state`.
fn from_the_state(from: usize, reply: impl Fn() -> Reply + Send + Sync + 'static) -> FakeClient {
    let states = AtomicUsize::new(0);
    answering(Level::Render, move |verb| match verb {
        "state" if states.fetch_add(1, Ordering::Relaxed) + 1 >= from => Some(reply()),
        "wait_until 1 chunks>=1" => Some(Reply::current(
            LATE,
            json!({"ok": false, "error": "the late answer"}),
        )),
        "clock real" => Some(Reply::current(SLOW, json!({"ok": true, "clock": "real"}))),
        _ => None,
    })
}

#[test]
fn the_clock_goes_back_past_a_connection_down_and_a_silent_state() {
    let fake = from_the_state(3, || Reply::current(NEVER_IN_TIME, state_of_a_game()));
    let dir = registry::home();
    fs::write(dir.join("s.scn"), "wait_until 1 chunks>=1\n").unwrap();
    assert_eq!(live_run(&dir, &[]), Err(String::new()));
    let report = fs::read_to_string(dir.join("out/report.txt")).unwrap();
    assert!(!report.contains("was not given back"), "{report}");
    assert!(!report.contains("the late answer"), "{report}");
    assert_eq!(
        fake.heard(),
        ["state", "state", "wait_until 1 chunks>=1", "clock real"]
    );
}

#[test]
fn a_reopening_has_the_limits_of_the_run() {
    let fake = from_the_state(3, || Reply::current(NEVER_IN_TIME, state_of_a_game()));
    let dir = registry::home();
    fs::write(dir.join("s.scn"), "wait_until 1 chunks>=1\nact map\n").unwrap();
    assert_eq!(live_run(&dir, &["--keep-going"]), Err(String::new()));
    let report = fs::read_to_string(dir.join("out/report.txt")).unwrap();
    assert!(
        report.contains("client fake does not answer: no reply to state within 600ms"),
        "{report}"
    );
    assert_eq!(
        fake.heard(),
        [
            "state",
            "state",
            "wait_until 1 chunks>=1",
            "state",
            "clock real"
        ]
    );
}

#[test]
fn the_clock_goes_back_to_a_probe_of_another_version_after_a_reopening() {
    let fake = from_the_state(3, || Reply {
        after: Duration::ZERO,
        line: r#"{"ok":true,"clock":"real","version":"0000000000000000"}"#.to_string(),
    });
    let dir = registry::home();
    fs::write(dir.join("s.scn"), "wait_until 1 chunks>=1\nact map\n").unwrap();
    assert_eq!(live_run(&dir, &["--keep-going"]), Err(String::new()));
    let report = fs::read_to_string(dir.join("out/report.txt")).unwrap();
    assert!(
        report.contains("runs probe version 0000000000000000"),
        "{report}"
    );
    assert!(!report.contains("was not given back"), "{report}");
    assert_eq!(
        fake.heard(),
        [
            "state",
            "state",
            "wait_until 1 chunks>=1",
            "state",
            "clock real"
        ]
    );
}

#[test]
fn a_probe_that_knows_no_clock_on_its_own_connection_is_told_to_the_report() {
    let fake = answering(Level::Render, |verb| match verb {
        "act a" => Some(garbled()),
        "clock real" => Some(Reply {
            after: Duration::ZERO,
            line: r#"{"ok":false,"error":"unknown verb clock"}"#.to_string(),
        }),
        _ => None,
    });
    let dir = registry::home();
    fs::write(dir.join("s.scn"), "act a\n").unwrap();
    assert_eq!(live_run(&dir, &[]), Err(String::new()));
    let report = fs::read_to_string(dir.join("out/report.txt")).unwrap();
    assert!(report.contains("was not given back"), "{report}");
    assert!(report.contains("unknown verb clock"), "{report}");
    assert!(report.contains("restart the client"), "{report}");
    assert!(!report.contains("voidc cmd fake"), "{report}");
    assert_eq!(fake.heard(), ["state", "state", "act a", "clock real"]);
}
