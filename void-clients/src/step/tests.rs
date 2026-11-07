use std::collections::VecDeque;
use std::os::unix::net::UnixStream;

use serde_json::json;

use super::*;
use crate::control::Failure;

#[derive(Default)]
struct Script {
    heard: Vec<String>,
    answers: VecDeque<Result<Value, Failure>>,
}

impl Ask for Script {
    fn ask(&mut self, line: &str) -> Result<Value, Failure> {
        self.heard.push(line.to_string());
        self.answers.pop_front().expect(line)
    }
}

fn script(answers: Vec<Result<Value, Failure>>) -> Script {
    Script {
        heard: Vec::new(),
        answers: answers.into(),
    }
}

fn plan(args: &[&str]) -> Result<Plan, String> {
    let mut all = vec!["--server", "s", "--out", "o"];
    all.extend(args);
    let all: Vec<String> = all.iter().map(|arg| arg.to_string()).collect();
    Plan::parse(&all, Level::Render)
}

fn sent(client_in: u64, out: u64) -> Result<Value, Failure> {
    Ok(json!({"in": client_in, "out": out, "active": true}))
}

#[test]
fn a_tick_passes_what_each_side_owes_the_other() {
    let mut lock = Lockstep {
        server: script(vec![Ok(json!({"tick": 7, "sent": 110, "received": 53}))]),
        client: script(vec![sent(40, 13), Ok(json!({"in": 50}))]),
        origin: Origin {
            sent: 100,
            received: 50,
            client_in: 40,
            client_out: 10,
        },
    };
    assert_eq!(lock.tick(), Ok(7));
    assert_eq!(lock.server.heard, ["step 1 53"]);
    assert_eq!(lock.client.heard, ["sent", "fence 50"]);
}

#[test]
fn a_client_cut_from_the_server_stops_the_passage() {
    let mut lock = Lockstep {
        server: script(vec![]),
        client: script(vec![Ok(json!({"in": 1, "out": 1, "active": false}))]),
        origin: Origin::default(),
    };
    assert!(lock.tick().unwrap_err().contains("lost its connection"));
    assert!(lock.server.heard.is_empty());
}

#[test]
fn an_expired_barrier_is_an_error_and_nothing_goes_on() {
    let mut lock = Lockstep {
        server: script(vec![Err(Failure::Refused(
            json!({"ok": false, "error": "fence expired: received 1 of 2"}),
        ))]),
        client: script(vec![sent(0, 2)]),
        origin: Origin::default(),
    };
    assert_eq!(
        lock.tick(),
        Err(r#"server: step 1 2: "fence expired: received 1 of 2""#.to_string())
    );
    assert_eq!(lock.client.heard, ["sent"]);
}

#[test]
fn calibration_waits_for_counters_that_stopped_moving() {
    let status = |sent: u64| Ok(json!({"sent": sent, "received": 5}));
    let mut server = vec![status(9)];
    server.extend((0..QUIET_POLLS + 1).map(|_| status(10)));
    let mut client = vec![sent(8, 3)];
    client.extend((0..QUIET_POLLS + 1).map(|_| sent(10, 3)));
    let mut lock = Lockstep {
        server: script(server),
        client: script(client),
        origin: Origin::default(),
    };
    lock.calibrate(Duration::ZERO).unwrap();
    assert_eq!(
        lock.origin,
        Origin {
            sent: 10,
            received: 5,
            client_in: 10,
            client_out: 3
        }
    );
}

#[test]
fn a_passage_writes_one_measure_per_frame_and_keeps_the_wanted_pictures() {
    let camera = || Ok(json!({"x": 1.5, "y": 2.0, "z": -3.25, "yaw": 90.0, "pitch": 0.5}));
    let mut client = vec![sent(0, 0), Ok(json!({"in": 0})), camera()];
    client.push(Ok(json!({"path": "/shots/f00000.png"})));
    client.push(camera());
    client.push(sent(0, 0));
    let mut lock = Lockstep {
        server: script(vec![Ok(json!({"tick": 1, "sent": 0, "received": 0}))]),
        client: script(client),
        origin: Origin::default(),
    };
    let plan = plan(&["--ticks", "1", "--frames", "2", "--every", "2"]).unwrap();
    let mut measures = Vec::new();
    let mut kept = Vec::new();
    let captured = play(&mut lock, &plan, &mut measures, |name, path| {
        kept.push((name.to_string(), path.to_path_buf()));
        Ok(())
    })
    .unwrap();
    assert_eq!(captured, 1);
    assert_eq!(kept[0].0, "f00000");
    assert!(lock.client.heard.contains(&"step 25".to_string()));
    let lines = String::from_utf8(measures).unwrap();
    assert_eq!(
        lines.lines().nth(1),
        Some(r#"{"t":0,"k":1,"x":1.5,"y":2.0,"z":-3.25,"yaw":90.0,"pitch":0.5}"#)
    );
    assert_eq!(lines.lines().count(), 2);
}

#[test]
fn a_client_cut_on_the_last_tick_fails_the_passage() {
    let client = vec![
        sent(0, 0),
        Ok(json!({"in": 0})),
        Ok(json!({"x": 0, "y": 0, "z": 0, "yaw": 0, "pitch": 0})),
        Ok(json!({"in": 0, "out": 0, "active": false})),
    ];
    let mut lock = Lockstep {
        server: script(vec![Ok(json!({"tick": 1, "sent": 0, "received": 0}))]),
        client: script(client),
        origin: Origin::default(),
    };
    let plan = plan(&["--ticks", "1", "--frames", "1", "--every", "0"]).unwrap();
    let error = play(&mut lock, &plan, &mut Vec::new(), |_, _| Ok(())).unwrap_err();
    assert!(error.contains("lost its connection"));
}

struct Fake {
    alive: bool,
    in_game: std::cell::RefCell<VecDeque<bool>>,
}

impl Joining for Fake {
    fn alive(&self) -> bool {
        self.alive
    }

    fn in_game(&self) -> bool {
        self.in_game.borrow_mut().pop_front().unwrap_or(false)
    }

    fn name(&self) -> &str {
        "fake"
    }

    fn min_chunks(&self) -> u64 {
        200
    }
}

fn fake(in_game: &[bool]) -> Fake {
    Fake {
        alive: true,
        in_game: std::cell::RefCell::new(in_game.iter().copied().collect()),
    }
}

fn ticks(from: u64, to: u64) -> Script {
    let mut answers: Vec<_> = vec![Ok(json!({"tick": from}))];
    answers.extend((from + 1..=to).map(|tick| Ok(json!({"tick": tick}))));
    script(answers)
}

#[test]
fn a_server_that_does_not_count_ends_the_wait() {
    let mut server = script(vec![Ok(json!({"tick": 0})), Ok(json!({"tick": 0}))]);
    let error = join(&mut server, &fake(&[]), 600, Duration::ZERO).unwrap_err();
    assert!(error.contains("does not count"));
}

#[test]
fn a_client_gone_from_the_game_at_the_lock_is_not_in_game() {
    let error = join(&mut ticks(0, 10), &fake(&[true, false]), 10, Duration::ZERO).unwrap_err();
    assert!(error.contains("not in game with 200 chunks at tick 10"));
    assert!(error.contains("--min-chunks"));
}

#[test]
fn one_slow_answer_at_the_lock_is_not_a_failure() {
    let client = fake(&[true, false, false, true]);
    assert_eq!(join(&mut ticks(0, 10), &client, 10, Duration::ZERO), Ok(()));
}

#[test]
fn the_lock_comes_with_the_client_in_game() {
    let client = fake(&[true, true]);
    assert_eq!(join(&mut ticks(0, 10), &client, 10, Duration::ZERO), Ok(()));
}

#[test]
fn a_server_past_the_lock_is_refused() {
    let mut server = script(vec![Ok(json!({"tick": 700}))]);
    let error = join(&mut server, &fake(&[]), 600, Duration::ZERO).unwrap_err();
    assert!(error.contains("restart it"));
}

#[test]
fn a_failed_passage_still_stops_the_server() {
    let mut server = script(vec![Ok(json!({"ok": true}))]);
    let result = finish(&mut server, true, Err("the client is gone".into()));
    assert_eq!(result, Err("the client is gone".to_string()));
    assert_eq!(server.heard, ["stop"]);
}

#[test]
fn a_stop_that_fails_is_told_after_the_failure_of_the_passage() {
    let mut server = script(vec![Err(Failure::Refused(
        json!({"ok": false, "error": "busy"}),
    ))]);
    let result = finish(&mut server, true, Err("the client is gone".into()));
    assert_eq!(
        result,
        Err("the client is gone\nserver: stop: \"busy\"".to_string())
    );
}

#[test]
fn a_server_that_lost_a_line_still_gets_its_stop_on_a_new_connection() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    let (home, path) = control::fake::test_socket("server");
    registry::set_test_home(None);
    let listener = UnixListener::bind(&path).unwrap();
    let (heard, lines) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for reply in ["not json", r#"{"ok":true}"#] {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            heard.send(line.trim_end().to_string()).unwrap();
            writeln!(&stream, "{reply}").unwrap();
        }
    });
    let mut server = Probe::to_socket("server", &path, Limits::REAL).unwrap();
    let lost = told(&mut server, "server", "step 1 3").unwrap_err();
    assert!(lost.contains("no JSON"), "{lost}");
    let result = finish(&mut server, true, Err(lost.clone()));
    let limit = Duration::from_secs(5);
    let first = lines.recv_timeout(limit);
    let second = lines.recv_timeout(limit);
    fs::remove_dir_all(home).unwrap();
    assert_eq!(result, Err(lost));
    assert_eq!(first.as_deref(), Ok("step 1 3"));
    assert_eq!(second.as_deref(), Ok("stop"));
}

#[test]
fn the_server_stays_unless_asked() {
    let mut server = script(vec![]);
    assert_eq!(finish(&mut server, false, Ok(())), Ok(()));
    assert!(server.heard.is_empty());
}

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("voidc-step-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_run_starts_without_what_the_last_one_wrote() {
    let dir = folder("prepare");
    for name in [
        "f00000.png",
        "f00479.png",
        "video.mp4",
        "sheet.png",
        "frames.jsonl",
    ] {
        fs::write(dir.join(name), b"old").unwrap();
    }
    fs::write(dir.join("notes.txt"), b"mine").unwrap();
    fs::write(dir.join("fond.png"), b"mine").unwrap();
    prepare(&dir).unwrap();
    let mut left: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, ["fond.png", "notes.txt"]);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn outputs_are_what_the_glob_of_the_video_takes() {
    assert!(is_output("f00012.png") && is_output("f1x.png"));
    assert!(!is_output("foo.png") && !is_output("f.png"));
}

#[test]
fn a_socket_has_thirty_seconds_for_real_to_answer() {
    use std::io::{BufRead, BufReader, Write};
    let (ours, theirs) = UnixStream::pair().unwrap();
    std::thread::spawn(move || {
        let mut line = String::new();
        BufReader::new(theirs.try_clone().unwrap())
            .read_line(&mut line)
            .unwrap();
        writeln!(&theirs, r#"{{"ok":true,"tick":0}}"#).unwrap();
    });
    let mut probe = Probe::over("server", ours, Limits::REAL).unwrap();
    probe.ask("status").unwrap();
    assert_eq!(probe.read_timeout(), Some(Duration::from_secs(30)));
    let verbs = [
        "status",
        "step 1 53",
        "stop",
        "sent",
        "fence 50",
        "step 12.5",
        "shot f00000",
    ];
    for verb in verbs.into_iter().chain(["clock virtual", "act tp 0 90 0"]) {
        assert_eq!(Limits::REAL.of(verb), Duration::from_secs(30), "{verb}");
    }
}

#[test]
fn the_counters_have_ten_seconds_for_real_to_fall_quiet() {
    assert_eq!(QUIET_POLL * QUIET_LIMIT, Duration::from_secs(10));
}

#[test]
fn a_silent_peer_is_named_in_the_error() {
    let (ours, _theirs) = UnixStream::pair().unwrap();
    let mut probe = Probe::over("client", ours, Limits::REAL).unwrap();
    let error = probe
        .ask_within("sent", Duration::from_millis(20))
        .unwrap_err();
    let error = error.to_string();
    assert!(
        error.contains("client does not answer: no reply to sent"),
        "{error}"
    );
}

#[test]
fn a_folder_with_a_bracket_still_makes_a_video_and_a_sheet() {
    let dir = folder("a[b]");
    let made = Command::new("ffmpeg")
        .current_dir(&dir)
        .args(["-y", "-loglevel", "error", "-f", "lavfi", "-i"])
        .args(["color=red:size=64x36:rate=1:duration=3", "f%05d.png"])
        .status();
    if !made.is_ok_and(|status| status.success()) {
        eprintln!("no ffmpeg: skipped");
        return;
    }
    fs::write(dir.join("foo.png"), b"not a picture").unwrap();
    assemble(&dir, 4, 3).unwrap();
    assert!(dir.join("video.mp4").exists() && dir.join("sheet.png").exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn hand_picked_pictures_play_slowly_and_a_stride_plays_at_game_speed() {
    assert_eq!(
        plan(&["--at", "3,7"]).unwrap().video_rate(),
        SPARSE_VIDEO_RATE
    );
    assert_eq!(plan(&["--every", "1"]).unwrap().video_rate(), 80);
    assert_eq!(plan(&["--every", "4"]).unwrap().video_rate(), 20);
}

#[test]
fn the_chunks_to_wait_for_are_an_option() {
    assert_eq!(plan(&[]).unwrap().min_chunks, MIN_CHUNKS);
    assert_eq!(plan(&["--min-chunks", "50"]).unwrap().min_chunks, 50);
}

#[test]
fn pictures_are_wanted_by_stride_or_by_index() {
    let every = plan(&["--every", "4"]).unwrap();
    assert!(every.wanted(0) && every.wanted(8) && !every.wanted(3));
    let some = plan(&["--at", "3,7"]).unwrap();
    assert!(some.wanted(7) && !some.wanted(0));
    let none = plan(&["--every", "0"]).unwrap();
    assert!(!none.captures());
}

#[test]
fn the_logic_level_cannot_capture() {
    let args: Vec<String> = ["--server", "s", "--out", "o"]
        .iter()
        .map(|arg| arg.to_string())
        .collect();
    assert!(Plan::parse(&args, Level::Logic).is_err());
    let mut none = args.clone();
    none.extend(["--every".to_string(), "0".to_string()]);
    assert!(Plan::parse(&none, Level::Logic).is_ok());
}

#[test]
fn a_plan_needs_a_server_a_folder_and_a_frame() {
    assert!(Plan::parse(&[], Level::Render).is_err());
    assert!(plan(&["--frames", "0"]).is_err());
    assert_eq!(plan(&["--hold", "30:45"]).unwrap().hold, Some((30, 45)));
    assert!(plan(&["--hold", "30"]).is_err());
}

#[test]
fn a_sheet_picks_evenly_and_never_more_than_it_holds() {
    assert!(sheet_filter(480).ends_with("tile=6x4"));
    assert!(sheet_filter(480).contains("mod(n\\,20)"));
    assert!(sheet_filter(5).ends_with("tile=5x1"));
    assert!(sheet_filter(25).contains("mod(n\\,2)"));
    assert!(sheet_filter(25).ends_with("tile=6x3"));
}

#[test]
fn every_act_is_kept_in_order() {
    let acts = plan(&["--act", "tp 80 90 0", "--act", "tell @r hi"])
        .unwrap()
        .acts;
    assert_eq!(acts, ["tp 80 90 0", "tell @r hi"]);
    assert!(plan(&[]).unwrap().acts.is_empty());
}

#[test]
fn step_refuses_a_probe_of_another_version() {
    let fake = control::fake::FakeClient::stale(Level::Render);
    let client = registry::load(control::fake::NAME).unwrap();
    let out = registry::home().join("out");
    let plan = plan(&["--lock-at", "10", "--min-chunks", "1"])
        .map(|plan| Plan { out, ..plan })
        .unwrap();
    let mut ticks = vec![Ok(json!({"tick": 0}))];
    ticks.extend((1..=10).map(|tick| Ok(json!({"tick": tick}))));
    fake.assert_refused(passage(&mut script(ticks), &client, &plan));
}
