use super::*;

#[test]
fn a_field_that_turns_out_to_be_text_is_refused_before_the_first_verb() {
    let dir = folder("live-text");
    let scenario = Scenario::parse("act map\nassert mood >= 3\n").unwrap();
    let mut fake = Fake::new(&dir, vec![]);
    fake.state["mood"] = json!("calm");
    let error = run(&dir, false).play(&scenario, &mut fake).unwrap_err();
    assert!(error.contains("line 2: mood is not a number"), "{error}");
    assert!(fake.asked.is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_client_coming_back_is_not_refused_for_the_fields_it_lacks_for_now() {
    let dir = folder("reconnect");
    let scenario = Scenario::parse("wait_until chunks>=1\n").unwrap();
    for state in [
        json!({"ok": true, "state": "connecting", "screen": "ConnectScreen", "clock": "real"}),
        json!({"ok": true, "state": "connected", "clock": "real"}),
        json!({"ok": false, "error": "game not ready"}),
    ] {
        let mut fake = Fake::new(&dir, vec![]);
        fake.state = state;
        assert!(run(&dir, false).play(&scenario, &mut fake).unwrap().passed);
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_camera_that_cannot_be_seen_is_not_a_still_camera() {
    let dir = folder("no-camera");
    let scenario = Scenario::parse("wait_still 10 120\n").unwrap();
    for state in [
        json!({"ok": true, "state": "connecting", "clock": "real"}),
        json!({"ok": false, "error": "game not ready"}),
    ] {
        let mut fake = Fake::new(&dir, vec![]);
        fake.state = state;
        let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
        assert!(!report.passed);
        let error = report.lines[0].error.as_ref().unwrap();
        assert!(error.contains("no camera"), "{error}");
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_failed_assert_gives_its_line_and_what_was_received() {
    let dir = folder("assert");
    let scenario = Scenario::parse("# open\nassert chunks>=900\nact map\n").unwrap();
    let refusal = json!({"ok": false, "error": "chunks>=900 is false: {...}"});
    let mut fake = Fake::new(&dir, vec![refusal]);
    let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
    assert!(!report.passed);
    assert_eq!((report.played, report.total), (1, 2));
    assert_eq!(report.lines[0].line, 2);
    assert_eq!(report.lines[0].got, Some(json!(71)));
    assert_eq!(fake.asked, ["assert chunks>=900", "clock real"]);
    assert!(report.text().contains("FAIL"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_clock_left_virtual_goes_back_to_real_even_after_a_failure() {
    let dir = folder("clock-left");
    let scenario = Scenario::parse("clock virtual\nassert chunks>=900\nact map\n").unwrap();
    let refusal = json!({"ok": false, "error": "chunks>=900 is false: {...}"});
    let mut fake = Fake::new(&dir, vec![json!({"ok": true}), refusal]);
    let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
    assert!(!report.passed);
    assert_eq!(report.played, 2);
    assert_eq!(fake.asked.last().unwrap(), "clock real");
    assert_eq!(fake.state["clock"], "real");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn every_run_on_a_game_gives_the_clock_back_whatever_it_played() {
    for (name, text) in [
        ("clock-back", "clock virtual\nclock real\n"),
        ("clock-never", "act map\n"),
        ("clock-real-only", "clock real\n"),
    ] {
        let dir = folder(name);
        let scenario = Scenario::parse(text).unwrap();
        let mut fake = Fake::new(&dir, vec![]);
        let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
        assert!(report.passed, "{}", report.text());
        assert_eq!(report.lines.len(), scenario.lines.len());
        assert_eq!(fake.asked.last().unwrap(), "clock real");
        let released = fake.asked.iter().filter(|verb| *verb == "clock real");
        assert_eq!(released.count(), text.matches("clock real").count() + 1);
        fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn a_clock_that_cannot_go_back_fails_the_run() {
    let dir = folder("clock-stuck");
    let scenario = Scenario::parse("clock virtual\n").unwrap();
    let refused = json!({"ok": false, "error": "no", "version": version::current()});
    let mut fake = Fake::new(&dir, vec![json!({"ok": true}), refused]);
    let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
    assert!(!report.passed);
    assert_eq!(report.failed, 1);
    let error = report.lines[1].error.as_ref().unwrap();
    assert!(
        error.contains("was not given back") && error.contains("voidc cmd test clock real"),
        "{error}"
    );
    assert!(report.text().contains("clock real, at the end of the run"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_recording_has_five_seconds_for_real_to_move() {
    let dir = folder("patience");
    let real = Run::new("test", Level::Render, dir.clone(), dir.join("out"), false);
    assert_eq!(real.patience, Duration::from_secs(5));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_client_gone_at_the_end_fails_the_run() {
    let dir = folder("clock-gone");
    let scenario = Scenario::parse("act map\n").unwrap();
    let mut fake = Fake::new(&dir, vec![]);
    fake.gone_at_release = true;
    let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
    assert!(!report.passed);
    assert_eq!(report.failed, 1);
    let error = report.lines.last().unwrap().error.as_ref().unwrap();
    assert!(
        error.contains("was not given back") && error.contains("connection refused"),
        "{error}"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_probe_that_knows_no_clock_fails_the_run_and_is_told_to_restart() {
    let dir = folder("clock-unknown");
    let scenario = Scenario::parse("act map\n").unwrap();
    let unknown = json!({"ok": false, "error": "unknown verb clock"});
    let mut fake = Fake::new(&dir, vec![json!({"ok": true}), unknown]);
    fake.state = json!({"ok": false, "error": "game not ready"});
    let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
    assert!(!report.passed);
    assert_eq!(fake.asked, ["act map", "clock real"]);
    let error = report.lines.last().unwrap().error.as_ref().unwrap();
    assert!(
        error.contains("unknown verb clock") && error.contains("restart the client"),
        "{error}"
    );
    assert!(!error.contains("voidc cmd test"), "{error}");
    assert!(error.contains("voidc down test"), "{error}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_probe_that_knows_no_clock_is_told_to_restart_at_its_first_clock_line_only() {
    let dir = folder("clock-unknown-reset");
    let scenario = Scenario::parse("clock real\nact map\n").unwrap();
    let unknown =
        json!({"ok": false, "error": "java.lang.IllegalArgumentException: unknown verb clock"});
    let mut fake = Fake::new(&dir, vec![unknown]);
    fake.state.as_object_mut().unwrap().remove("clock");
    let report = run(&dir, true).play(&scenario, &mut fake).unwrap();
    assert!(!report.passed);
    assert_eq!(report.failed, 1, "{}", report.text());
    let error = report.lines[0].error.as_ref().unwrap();
    assert!(
        error.contains("unknown verb clock") && error.contains("Restart the client"),
        "{error}"
    );
    assert_eq!(fake.asked, ["clock real", "act map"]);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_failed_clock_real_is_given_back_at_the_end_all_the_same() {
    let dir = folder("clock-real-failed");
    let scenario = Scenario::parse("clock virtual\nclock real\n").unwrap();
    let refused = json!({"ok": false, "error": "no", "version": version::current()});
    let mut fake = Fake::new(&dir, vec![json!({"ok": true}), refused]);
    let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
    assert!(!report.passed);
    assert_eq!(fake.asked, ["clock virtual", "clock real", "clock real"]);
    assert_eq!(fake.state["clock"], "real");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_run_stopped_before_its_clock_line_still_gives_the_clock_back() {
    let dir = folder("clock-unplayed");
    let scenario = Scenario::parse("assert chunks>=900\nclock virtual\n").unwrap();
    let refusal = json!({"ok": false, "error": "chunks>=900 is false: {...}"});
    let mut fake = Fake::new(&dir, vec![refusal]);
    let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
    assert_eq!((report.played, report.lines.len()), (1, 1));
    assert_eq!(
        fake.log,
        ["state", "assert chunks>=900", "state", "clock real"]
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_clock_is_given_back_only_once_the_recordings_are_in() {
    let dir = folder("clock-after-recording");
    let scenario = Scenario::parse("clock virtual\nframes 2\n").unwrap();
    let mut fake = Fake::new(&dir, vec![]);
    fake.frames = "1000000 1 2 70 12 true 0 0\n1010000 1 2 70 12 true 0 0\n".into();
    let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
    assert!(report.passed, "{}", report.text());
    assert_eq!(fake.collected_when_released, [true]);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_recording_the_probe_marked_invalid_fails_the_run_and_is_not_kept() {
    let source = include_str!("../../../probe/Probe.java");
    let (_, rest) = source.split_once("SPOILED = \"").unwrap();
    let (spoiled, _) = rest.split_once('"').unwrap();
    for (name, text) in [
        ("invalid-unchecked", "clock virtual\nframes 2\nclock real\n"),
        (
            "invalid-checked",
            "clock virtual\nframes 2\nclock real\ncheck frames\n",
        ),
    ] {
        let dir = folder(name);
        fs::create_dir_all(dir.join("out")).unwrap();
        fs::write(dir.join("out/frames.txt"), "1 2 3\n").unwrap();
        let scenario = Scenario::parse(text).unwrap();
        scenario.validate(Level::Render, &dir).unwrap();
        let mut fake = Fake::new(&dir, vec![]);
        fake.frames =
            format!("1000000 1 2 70 12 true 0 0\n1010000 1 2 70 12 true 0 0\n{spoiled}\n");
        let report = run(&dir, true).play(&scenario, &mut fake).unwrap();
        assert!(!report.passed, "{name}: {}", report.text());
        let error = report
            .lines
            .iter()
            .find_map(|line| line.error.as_ref())
            .unwrap();
        assert!(
            error.contains("is invalid") && error.contains("went back to real time"),
            "{name}: {error}"
        );
        assert!(!dir.join("out/frames.txt").exists(), "{name}");
        fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn what_is_kept_is_the_text_that_was_read() {
    let dir = folder("kept-as-read");
    let scenario = Scenario::parse("frames 2\n").unwrap();
    let mut fake = Fake::new(&dir, vec![]);
    fake.frames = "1000000 1 2 70 12 true 0 0\n1010000 1 2 70 12 true 0 0\n".into();
    fake.rewritten_after_read = true;
    let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
    assert!(report.passed, "{}", report.text());
    assert_eq!(
        fs::read_to_string(dir.join("out/frames.txt")).unwrap(),
        fake.frames
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_bot_is_neither_asked_its_clock_nor_given_it_back() {
    let dir = folder("clock-bot");
    let scenario = Scenario::parse("act map\n").unwrap();
    let mut fake = Fake::new(&dir, vec![]);
    fake.state.as_object_mut().unwrap().remove("clock");
    let mut bot = run(&dir, false);
    bot.level = Level::Bot;
    let report = bot.play(&scenario, &mut fake).unwrap();
    assert!(report.passed, "{}", report.text());
    assert_eq!(fake.asked, ["act map"]);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_clock_found_virtual_stops_the_run_before_its_first_verb() {
    let dir = folder("clock-found");
    let scenario = Scenario::parse("clock virtual\nact map\n").unwrap();
    let mut fake = Fake::new(&dir, vec![]);
    fake.state["clock"] = json!("virtual");
    fake.state["state"] = json!("connecting");
    let error = run(&dir, false).play(&scenario, &mut fake).unwrap_err();
    assert!(error.contains("voidc cmd test clock real"), "{error}");
    assert!(fake.asked.is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_client_that_does_not_tell_its_clock_gets_it_back_without_a_guess() {
    let dir = folder("clock-untold");
    let text = "clock real\nclock virtual\nact map\nclock real\n";
    let scenario = Scenario::parse(text).unwrap();
    let mut fake = Fake::new(&dir, vec![]);
    fake.state.as_object_mut().unwrap().remove("clock");
    fake.untold = true;
    let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
    assert!(report.passed, "{}", report.text());
    assert_eq!(
        fake.asked,
        [
            "clock real",
            "clock virtual",
            "act map",
            "clock real",
            "clock real"
        ]
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_client_that_does_not_tell_its_clock_is_refused_unless_the_scenario_resets_it() {
    let dir = folder("clock-untold-preflight");
    let mut fake = Fake::new(&dir, vec![]);
    fake.state.as_object_mut().unwrap().remove("clock");
    fake.untold = true;
    let scenario = Scenario::parse("act map\n").unwrap();
    let error = run(&dir, false).play(&scenario, &mut fake).unwrap_err();
    assert!(
        error.contains("does not tell its clock") && error.contains("Restart the client"),
        "{error}"
    );
    assert!(!error.contains("cmd test clock real"), "{error}");
    assert!(fake.asked.is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_scenario_that_starts_with_clock_real_may_take_a_frozen_client() {
    let dir = folder("clock-reset");
    let scenario = Scenario::parse("clock real\nact map\n").unwrap();
    let mut fake = Fake::new(&dir, vec![]);
    fake.state["clock"] = json!("virtual");
    let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
    assert!(report.passed, "{}", report.text());
    assert_eq!(fake.asked, ["clock real", "act map", "clock real"]);
    assert_eq!(fake.state["clock"], "real");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn keep_going_plays_every_line_and_still_fails() {
    let dir = folder("keep");
    let scenario = Scenario::parse("assert chunks>=900\nact map\n").unwrap();
    let refusal = json!({"ok": false, "error": "no"});
    let mut fake = Fake::new(&dir, vec![refusal]);
    let report = run(&dir, true).play(&scenario, &mut fake).unwrap();
    assert_eq!(report.played, 2);
    assert_eq!(report.failed, 1);
    assert!(!report.passed);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_wait_always_carries_its_limit_so_a_value_is_never_taken_for_one() {
    let dir = folder("wait");
    let scenario = Scenario::parse("wait_until chunks >= 200\nwait_until 500 chunks>=1\n").unwrap();
    let mut fake = Fake::new(&dir, vec![]);
    run(&dir, false).play(&scenario, &mut fake).unwrap();
    assert_eq!(
        fake.asked,
        [
            "wait_until 20000 chunks >= 200",
            "wait_until 500 chunks>=1",
            "clock real"
        ]
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_field_no_client_has_stops_the_run_before_its_first_verb() {
    let dir = folder("field");
    let scenario = Scenario::parse("act map\nwait_until chunkz>=1\n").unwrap();
    let mut fake = Fake::new(&dir, vec![]);
    let error = run(&dir, false).play(&scenario, &mut fake).unwrap_err();
    assert!(error.contains("line 2: no field chunkz"));
    assert!(fake.asked.is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_shot_is_kept_and_compared_by_its_name() {
    let dir = folder("shot");
    let taken = dir.join("taken.png");
    let picture = compare::Picture {
        width: 4,
        height: 4,
        rgb: vec![9; 48],
    };
    picture.save(&taken).unwrap();
    picture.save(&dir.join("ref.png")).unwrap();
    let scenario = Scenario::parse("shot a\ncompare a ref.png max=0\n").unwrap();
    let reply = json!({"ok": true, "path": taken});
    let mut fake = Fake::new(&dir, vec![json!({"ok": true}), reply]);
    let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
    assert!(report.passed, "{}", report.text());
    assert_eq!(fake.asked[0], "wait_until 20000 loading==false");
    assert!(dir.join("out/shots/a.png").exists());
    assert!(dir.join("out/diffs/2-a-ref.png").exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_recording_runs_during_the_next_lines_and_is_kept_when_checked() {
    let dir = folder("frames");
    fs::write(dir.join("frames.txt"), "stale").unwrap();
    let scenario = Scenario::parse("frames 2\nact map\ncheck frames\n").unwrap();
    let mut fake = Fake::new(&dir, vec![]);
    fake.frames = "1000000 1 2 70 12 true 0 0\n1010000 90 2 70 12 true 0 0\n".into();
    let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
    assert_eq!(fake.asked, ["frames 2", "act map", "clock real"]);
    assert_eq!(fake.stale_when_asked, [false]);
    assert!(!report.passed);
    let error = report.lines[2].error.as_ref().unwrap();
    assert!(error.contains("jump past 64 blocks"), "{error}");
    assert!(
        fs::read_to_string(dir.join("out/frames.txt"))
            .unwrap()
            .starts_with("1000000")
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_recording_that_never_ends_fails_with_what_it_got() {
    let dir = folder("unfinished");
    let scenario = Scenario::parse("frames 500\ncheck frames\n").unwrap();
    let mut fake = Fake::new(&dir, vec![]);
    fake.frames = "1000000 1 2 70 12 true 0 0\n".into();
    fake.finishes = false;
    let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
    let error = report.lines[1].error.as_ref().unwrap();
    assert!(
        error.contains("500") && error.contains("1 lines"),
        "{error}"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_truncated_recording_fails_the_run_even_if_nothing_checks_it() {
    let dir = folder("unchecked");
    let scenario = Scenario::parse("frames 500\n").unwrap();
    let mut fake = Fake::new(&dir, vec![]);
    fake.frames = "1000000 1 2 70 12 true 0 0\n".into();
    let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
    assert!(!report.passed);
    assert_eq!((report.played, report.failed), (1, 1));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn track_waits_for_the_end_of_the_writing_and_not_for_a_first_line() {
    let dir = folder("track");
    let path = dir.join("track.txt");
    fs::write(&path, "1000 1 2\n").unwrap();
    let recording = Recording {
        line: 1,
        text: "track 10".into(),
        lines: None,
        patience: Duration::from_millis(60),
    };
    let error = wait_for_recording("track", &path, &recording).unwrap_err();
    assert!(error.contains("its end"), "{error}");
    fs::write(dir.join("track.done"), "").unwrap();
    assert!(wait_for_recording("track", &path, &recording).is_ok());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_half_written_last_line_does_not_count() {
    let dir = folder("half");
    let path = dir.join("frames.txt");
    fs::write(&path, "1 2 3\n4 5").unwrap();
    assert_eq!(line_count(&path), 1);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn what_a_green_check_did_not_look_at_is_said() {
    let dir = folder("green");
    let scenario = Scenario::parse("frames 2\ncheck frames\n").unwrap();
    let mut fake = Fake::new(&dir, vec![]);
    fake.frames = "1000000 1 2 70 12 true 0 0\n1010000 1 2 70 12 true 0 0\n".into();
    let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
    assert!(report.passed, "{}", report.text());
    assert!(
        report.text().contains("not checked: flashes"),
        "{}",
        report.text()
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_recording_nobody_checked_is_still_kept() {
    let dir = folder("kept");
    let scenario = Scenario::parse("frames 1\n").unwrap();
    let mut fake = Fake::new(&dir, vec![]);
    fake.frames = "1000000 1 2 70 12 true 0 0\n".into();
    run(&dir, false).play(&scenario, &mut fake).unwrap();
    assert!(dir.join("out/frames.txt").exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn checking_a_recording_nobody_made_is_refused() {
    let scenario = Scenario::parse("act map\ncheck frames\n").unwrap();
    let error = scenario
        .validate(Level::Render, Path::new("."))
        .expect_err("nothing records frames here");
    assert!(error.contains("line 2"), "{error}");
}

#[test]
fn an_old_recording_of_the_client_is_never_read() {
    let dir = folder("old-recording");
    fs::write(dir.join("frames.txt"), "1000000 1 2 70 12 true 0 0\n").unwrap();
    let scenario = Scenario::parse("check frames\n").unwrap();
    let mut fake = Fake::new(&dir, vec![]);
    let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
    assert!(!report.passed);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_truncated_recording_fails_with_both_counts() {
    let dir = folder("truncated");
    let scenario = Scenario::parse("frames 500\ncheck frames\n").unwrap();
    let mut fake = Fake::new(&dir, vec![]);
    fake.frames = "1000000 1 2 70 12 true 0 0\n1010000 1 2 70 12 true 0 0\n".into();
    let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
    assert!(!report.passed);
    let error = report.lines[1].error.as_ref().unwrap();
    assert!(error.contains("500") && error.contains('2'), "{error}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_reused_out_folder_gives_no_old_shot() {
    let dir = folder("stale-shot");
    fs::create_dir_all(dir.join("out/shots")).unwrap();
    let picture = compare::Picture {
        width: 4,
        height: 4,
        rgb: vec![9; 48],
    };
    picture.save(&dir.join("out/shots/a.png")).unwrap();
    picture.save(&dir.join("ref.png")).unwrap();
    let scenario = Scenario::parse("shot a\ncompare a ref.png max=0\n").unwrap();
    let refused = json!({"ok": false, "error": "no picture"});
    let mut fake = Fake::new(&dir, vec![json!({"ok": true}), refused]);
    let report = run(&dir, true).play(&scenario, &mut fake).unwrap();
    assert_eq!(report.failed, 2, "{}", report.text());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_reused_out_folder_gives_no_old_recording() {
    let dir = folder("stale-frames");
    fs::create_dir_all(dir.join("out")).unwrap();
    fs::write(dir.join("out/frames.txt"), "1000000 1 2 70 12 true 0 0\n").unwrap();
    let scenario = Scenario::parse("frames 1\ncheck frames\n").unwrap();
    let refused = json!({"ok": false, "error": "not now"});
    let mut fake = Fake::new(&dir, vec![refused]);
    let report = run(&dir, true).play(&scenario, &mut fake).unwrap();
    assert_eq!(report.failed, 2, "{}", report.text());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_second_recording_waits_for_the_first_to_be_checked() {
    for verb in ["frames", "track"] {
        let twice = format!("{verb} 10\nact map\n{verb} 10\ncheck {verb}\n");
        let scenario = Scenario::parse(&twice).unwrap();
        let error = scenario
            .validate(Level::Render, Path::new("."))
            .unwrap_err();
        assert!(error.contains("line 3"), "{error}");
        let checked = format!("{verb} 10\ncheck {verb}\n{verb} 10\ncheck {verb}\n");
        let scenario = Scenario::parse(&checked).unwrap();
        scenario.validate(Level::Render, Path::new(".")).unwrap();
    }
    let both = Scenario::parse("frames 10\ntrack 10\ncheck frames\ncheck track\n").unwrap();
    both.validate(Level::Render, Path::new(".")).unwrap();
}

#[test]
fn a_second_recording_refused_leaves_nothing_of_the_first_to_check() {
    let dir = folder("second-refused");
    let text = "frames 1\ncheck frames\nframes 1\ncheck frames\n";
    let scenario = Scenario::parse(text).unwrap();
    let refused = json!({"ok": false, "error": "not now"});
    let mut fake = Fake::new(&dir, vec![json!({"ok": true}), refused]);
    fake.frames = "1000000 1 2 70 12 true 0 0\n".into();
    let report = run(&dir, true).play(&scenario, &mut fake).unwrap();
    assert!(report.lines[1].passed, "{}", report.text());
    let error = report.lines[3]
        .error
        .as_ref()
        .expect("the first was checked again");
    assert!(error.contains("not recorded"), "{error}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_recording_still_running_refuses_the_next_and_gives_it_nothing() {
    let dir = folder("second-early");
    let text = "frames 1\ncheck frames\nframes 1\ncheck frames\n";
    let scenario = Scenario::parse(text).unwrap();
    let refused = json!({"ok": false, "error": "frames is still recording"});
    let mut fake = Fake::new(&dir, vec![json!({"ok": true}), refused]);
    fake.finishes = false;
    let report = run(&dir, true).play(&scenario, &mut fake).unwrap();
    assert!(report.lines[1].error.as_ref().unwrap().contains("stopped"));
    assert!(
        report.lines[2]
            .error
            .as_ref()
            .unwrap()
            .contains("still recording")
    );
    let error = report.lines[3]
        .error
        .as_ref()
        .expect("nothing was recorded");
    assert!(error.contains("not recorded"), "{error}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_far_jump_across_missing_ticks_reaches_the_report_and_passes() {
    let dir = folder("gap-warning");
    let scenario = Scenario::parse("frames 2\ncheck frames\n").unwrap();
    let mut fake = Fake::new(&dir, vec![]);
    fake.frames = "1000000 0 0 70 12 true 0 0\n1200000 70 0 70 12 true 0 0\n".into();
    let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
    assert!(report.lines[1].passed, "{}", report.text());
    assert!(
        report.text().contains("WARN 70.00 blocks"),
        "{}",
        report.text()
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_second_recording_truncated_leaves_nothing_of_the_first_to_check() {
    let dir = folder("second-short");
    let text = "frames 1\ncheck frames\nframes 5\ncheck frames\ncheck frames\n";
    let scenario = Scenario::parse(text).unwrap();
    let mut fake = Fake::new(&dir, vec![]);
    fake.frames = "1000000 1 2 70 12 true 0 0\n".into();
    let report = run(&dir, true).play(&scenario, &mut fake).unwrap();
    assert!(
        report.lines[3]
            .error
            .as_ref()
            .unwrap()
            .contains("truncated")
    );
    let error = report.lines[4]
        .error
        .as_ref()
        .expect("the first was checked again");
    assert!(error.contains("not recorded"), "{error}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_camera_that_keeps_moving_fails_and_a_still_one_passes() {
    let dir = folder("still");
    let scenario = Scenario::parse("wait_still 10 200\n").unwrap();
    let mut fake = Fake::new(&dir, vec![]);
    fake.state["cam_x"] = json!(1.0);
    assert!(run(&dir, false).play(&scenario, &mut fake).unwrap().passed);
    fake.moving = true;
    let report = run(&dir, false).play(&scenario, &mut fake).unwrap();
    assert!(
        report.lines[0]
            .error
            .as_ref()
            .unwrap()
            .contains("did not stop")
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_report_is_written_in_both_forms() {
    let dir = folder("report");
    let scenario = Scenario::parse("act map\n").unwrap();
    let mut fake = Fake::new(&dir, vec![]);
    let the_run = run(&dir, false);
    let report = the_run.play(&scenario, &mut fake).unwrap();
    report.write(&the_run.out).unwrap();
    let json: Value =
        serde_json::from_slice(&fs::read(dir.join("out/report.json")).unwrap()).unwrap();
    assert_eq!(json["passed"], true);
    assert_eq!(json["lines"][0]["line"], 1);
    let text = fs::read_to_string(dir.join("out/report.txt")).unwrap();
    assert!(text.contains("PASSED"));
    fs::remove_dir_all(dir).unwrap();
}
