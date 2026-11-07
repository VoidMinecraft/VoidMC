use std::fs;

use super::*;
use crate::hangup::{hang_up, held_by_a_child};

#[test]
fn a_wait_outlives_its_own_limit() {
    let timeout_of = |verb| Limits::REAL.of(verb);
    assert_eq!(
        timeout_of("wait_until 60000 chunks>=1"),
        Duration::from_secs(65)
    );
    assert_eq!(timeout_of("wait_until chunks>=1"), Duration::from_secs(25));
    assert_eq!(
        timeout_of("wait_until chunks >= 100"),
        Duration::from_secs(25)
    );
    assert_eq!(
        timeout_of("wait_until\t40000\tchunks>=1"),
        Duration::from_secs(45)
    );
    assert_eq!(timeout_of("state"), Duration::from_secs(30));
}

#[test]
fn a_state_outlives_what_the_probe_gives_the_game_thread() {
    assert!(include_str!("../../probe/Probe.java").contains("onGame(task, 10_000)"));
    assert!(Limits::REAL.open > Duration::from_secs(10));
}

#[test]
fn the_clock_given_back_has_thirty_seconds_for_real() {
    assert_eq!(Limits::REAL.of("clock real"), Duration::from_secs(30));
}

#[test]
fn the_limits_of_the_tests_are_the_real_ones_divided_by_twenty() {
    assert_eq!(Limits::TEST.open * 20, Limits::REAL.open);
    assert_eq!(Limits::TEST.of("state") * 20, Limits::REAL.of("state"));
    assert_eq!(Limits::TEST.past_a_wait * 20, Limits::REAL.past_a_wait);
}

#[test]
fn a_connection_the_client_hung_up_says_so() {
    use std::os::unix::net::UnixListener;
    let (home, socket) = fake::test_socket("gone");
    let listener = UnixListener::bind(socket).unwrap();
    let mut probe = Probe::unchecked("gone").unwrap();
    let (accepted, _) = listener.accept().unwrap();
    let _child = held_by_a_child(&accepted);
    hang_up(accepted);
    let error = probe
        .ask_within("act map", Duration::from_secs(1))
        .unwrap_err()
        .to_string();
    registry::set_test_home(None);
    fs::remove_dir_all(home).unwrap();
    assert!(
        error.contains("client gone closed the connection"),
        "{error}"
    );
}

#[test]
fn a_fake_client_leaves_nothing_behind() {
    let fake = fake::FakeClient::stale(registry::Level::Render);
    let client = registry::load(fake::NAME).unwrap();
    assert!(registry::alive(&client));
    drop(fake);
    assert!(!registry::alive(&client));
}

#[test]
fn a_long_tmpdir_leaves_the_fake_client_standing() {
    let run = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "tests::cmd_refuses_a_probe_of_another_version"])
        .env("TMPDIR", std::env::temp_dir().join("t".repeat(120)))
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&run.stdout);
    assert!(run.status.success() && out.contains("1 passed"), "{out}");
}

#[test]
fn a_line_break_cannot_slip_a_second_verb_in() {
    let error = ask("nobody", "assert chat~ok\nquit")
        .unwrap_err()
        .to_string();
    assert!(error.contains("line break"), "{error}");
}
