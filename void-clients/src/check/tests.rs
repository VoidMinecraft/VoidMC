use super::*;

fn line(t: u64, k: u32, x: f64, yaw: f64) -> Frame {
    Frame {
        t,
        k,
        x,
        y: Some(64.0),
        z: 0.0,
        yaw: Some(yaw),
        pitch: Some(10.0),
    }
}

fn limits(words: &[&str]) -> Limits {
    let words: Vec<String> = words.iter().map(|word| word.to_string()).collect();
    Limits::parse(&words, Path::new(".")).unwrap()
}

fn smooth() -> Vec<Frame> {
    (0..40)
        .map(|i| line(i / 4, (i % 4) as u32, i as f64, i as f64))
        .collect()
}

#[test]
fn a_smooth_flight_passes() {
    let outcome = check(&smooth(), &limits(&[])).unwrap();
    assert!(outcome.passed, "{}", outcome.summary());
}

#[test]
fn a_jump_past_64_blocks_is_an_error_of_its_own() {
    let mut frames = smooth();
    frames[20..].iter_mut().for_each(|frame| frame.x += 2593.0);
    let outcome = check(&frames, &limits(&[])).unwrap();
    assert!(!outcome.passed);
    assert_eq!(outcome.findings.len(), 1);
    let finding = &outcome.findings[0];
    assert_eq!(
        (finding.kind, finding.t, finding.k),
        ("jump past 64 blocks", 5, 0)
    );
    assert!(finding.value > 2593.0);
}

#[test]
fn a_smaller_jump_is_still_a_jump_and_a_long_gap_allows_more() {
    let mut frames = smooth();
    frames[20..].iter_mut().for_each(|frame| frame.x += 30.0);
    let outcome = check(&frames, &limits(&[])).unwrap();
    assert_eq!(outcome.findings[0].kind, "camera jump");
    let gap = [line(0, 0, 0.0, 0.0), line(4, 0, 40.0, 0.0)];
    assert!(check(&gap, &limits(&[])).unwrap().passed);
}

#[test]
fn a_half_turn_counts_the_short_way_round() {
    assert_eq!(turned(179.0, -179.0), 2.0);
    assert_eq!(turned(0.0, 720.0), 0.0);
    let across = [line(0, 0, 0.0, 179.0), line(0, 1, 0.0, -179.0)];
    assert!(check(&across, &limits(&[])).unwrap().passed);
    let back = [line(0, 0, 0.0, 10.0), line(0, 1, 0.0, 190.0)];
    let outcome = check(&back, &limits(&[])).unwrap();
    assert_eq!(outcome.findings[0].kind, "half turn");
}

#[test]
fn a_declared_cut_is_expected_and_any_other_jump_is_not() {
    let mut frames = smooth();
    frames[20..].iter_mut().for_each(|frame| frame.x += 2593.0);
    frames[32..].iter_mut().for_each(|frame| frame.x += 90.0);
    let outcome = check(&frames, &limits(&["cut=5"])).unwrap();
    assert!(outcome.findings[0].expected);
    assert!(!outcome.findings[1].expected);
    assert!(!outcome.passed);
    assert!(
        check(&frames, &limits(&["cut=5", "cut=8:0"]))
            .unwrap()
            .passed
    );
    assert!(check(&frames, &limits(&["cut=5-8"])).unwrap().passed);
}

#[test]
fn a_cut_that_never_happens_is_an_error() {
    let outcome = check(&smooth(), &limits(&["cut=3"])).unwrap();
    assert!(!outcome.passed);
    assert_eq!(outcome.unused_cuts, ["3"]);
}

#[test]
fn a_cut_on_one_image_leaves_the_rest_of_the_tick_out() {
    let cut = Cut::parse("5:2").unwrap();
    assert!(cut.covers(&line(5, 2, 0.0, 0.0)));
    assert!(!cut.covers(&line(5, 3, 0.0, 0.0)));
    assert!(Cut::parse("5-").is_err());
    assert!(Cut::parse("9-5").is_err());
}

#[test]
fn the_jsonl_of_a_stepped_run_is_read() {
    let frames = parse_frames(
        "{\"t\":661,\"k\":0,\"x\":1.5,\"y\":70.0,\"z\":2.0,\"yaw\":90.0,\"pitch\":5.0}\n\
         {\"t\":661,\"k\":1,\"x\":1.6,\"y\":70.0,\"z\":2.0,\"yaw\":91.0,\"pitch\":5.0}\n",
    )
    .unwrap();
    assert_eq!(frames.len(), 2);
    assert_eq!(
        (frames[1].t, frames[1].k, frames[1].yaw),
        (661, 1, Some(91.0))
    );
}

#[test]
fn the_frames_verb_gives_ticks_from_its_clock() {
    let frames = parse_frames(
        "1000000 1.0 2.0 70.0 12 true 90.0 5.0\n\
         1010000 1.0 2.0 70.0 12 true 90.0 5.0\n\
         1060000 1.5 2.0 70.0 12 false 90.0 5.0\n",
    )
    .unwrap();
    assert_eq!(
        frames.iter().map(|f| (f.t, f.k)).collect::<Vec<_>>(),
        [(0, 0), (0, 1), (1, 0)]
    );
    assert_eq!((frames[2].y, frames[2].z), (Some(70.0), 2.0));
}

#[test]
fn the_track_verb_has_no_angles_and_says_so() {
    let frames = parse_frames("1000 1.0 2.0\n1050 90.0 2.0\n").unwrap();
    assert_eq!(frames[1].t, 1);
    let outcome = check(&frames, &limits(&[])).unwrap();
    assert_eq!(outcome.findings[0].kind, "jump past 64 blocks");
    assert!(outcome.summary().contains("no angles"));
}

#[test]
fn a_step_past_64_blocks_counts_against_the_time_it_took() {
    let slow = [line(0, 0, 0.0, 0.0), line(10, 0, 100.0, 0.0)];
    assert!(check(&slow, &limits(&[])).unwrap().passed);
    let fast = [line(0, 0, 0.0, 0.0), line(1, 0, 100.0, 0.0)];
    let outcome = check(&fast, &limits(&[])).unwrap();
    assert_eq!(outcome.findings[0].kind, "jump past 64 blocks");
}

#[test]
fn not_a_number_is_refused_in_a_frame_and_in_a_limit() {
    let mut frames = smooth();
    frames[3].x = f64::NAN;
    assert!(
        check(&frames, &limits(&[]))
            .unwrap_err()
            .contains("not a number")
    );
    frames[3].x = 0.0;
    frames[5].yaw = Some(f64::INFINITY);
    assert!(check(&frames, &limits(&[])).is_err());
    let parsed = parse_frames("1000000 NaN 2.0 70.0 12 true 90.0 5.0\n").unwrap();
    assert!(check(&parsed, &limits(&[])).is_err());
    assert!(parse_frames("NaN 1.0 2.0\n").is_err());
    for word in ["jump=NaN", "turn=inf", "flash=0", "jump=-1"] {
        assert!(
            Limits::parse(&[word.to_string()], Path::new(".")).is_err(),
            "{word}"
        );
    }
}

#[test]
fn a_recording_the_probe_marked_invalid_is_refused_with_its_cause() {
    let source = include_str!("../../probe/Probe.java");
    let (_, rest) = source.split_once("SPOILED = \"").unwrap();
    let (probe, _) = rest.split_once('"').unwrap();
    let dir = std::env::temp_dir().join(format!("voidc-invalid-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("frames.txt");
    for text in [
        format!("1000000 1 2 70 12 true 0 0\n2000000 1 2 70 12 true 0 0\n{probe}\n"),
        format!("1000 1.0 2.0\n{probe}\n"),
    ] {
        fs::write(&path, &text).unwrap();
        for error in [
            read(&path).unwrap_err(),
            file(&path, &limits(&[])).unwrap_err(),
        ] {
            assert!(
                error.contains("is invalid") && error.contains("went back to real time"),
                "{error}"
            );
        }
    }
    fs::write(&path, "1000 1.0 2.0\n1050 1.0 2.0\n").unwrap();
    assert!(read(&path).is_ok());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_clock_that_goes_back_is_an_error_and_no_panic() {
    let error = parse_frames("1000 1.0 2.0\n900 1.0 2.0\n").unwrap_err();
    assert!(
        error.contains("line 2") && error.contains("clock verb"),
        "{error}"
    );
    let error = parse_frames("1000 1.0 2.0\n1200 1.0 2.0\n1100 1.0 2.0\n").unwrap_err();
    assert!(
        error.contains("line 3") && error.contains("clock verb"),
        "{error}"
    );
    let stepped =
        parse_frames("{\"t\":2,\"x\":1.0,\"z\":0.0}\n{\"t\":1,\"x\":1.0,\"z\":0.0}\n").unwrap();
    let error = check(&stepped, &limits(&[])).unwrap_err();
    assert!(
        error.contains("out of order") && !error.contains("clock"),
        "{error}"
    );
}

#[test]
fn frames_all_at_one_instant_warn_that_the_clock_stood_still() {
    let dir = std::env::temp_dir().join(format!("voidc-frozen-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("frames.txt");
    fs::write(&path, "5000 1 2 70 12 true 0 0\n5000 1 2 70 12 true 0 0\n").unwrap();
    let outcome = file(&path, &limits(&[])).unwrap();
    assert!(outcome.passed);
    assert!(
        outcome
            .note()
            .unwrap()
            .contains("WARN all 2 frames fall at the same instant")
    );
    fs::write(&path, "5000 1 2 70 12 true 0 0\n5100 1 2 70 12 true 0 0\n").unwrap();
    let outcome = file(&path, &limits(&[])).unwrap();
    assert!(outcome.warnings.is_empty());
    fs::write(
        &path,
        "{\"t\":1,\"x\":1.0,\"z\":0.0}\n{\"t\":1,\"k\":1,\"x\":1.0,\"z\":0.0}\n",
    )
    .unwrap();
    assert!(file(&path, &limits(&[])).unwrap().warnings.is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_far_jump_across_missing_ticks_warns_and_passes() {
    let gap = [line(10, 0, 0.0, 0.0), line(14, 0, 70.0, 0.0)];
    let outcome = check(&gap, &limits(&[])).unwrap();
    assert!(outcome.passed);
    assert_eq!(outcome.warnings.len(), 1);
    assert!(outcome.warnings[0].contains("70.00 blocks"));
    assert!(outcome.warnings[0].contains("gap of 4 ticks"));
    assert!(outcome.summary().contains("WARN 70.00 blocks"));
    let json = serde_json::to_value(&outcome).unwrap();
    assert_eq!(json["warnings"].as_array().unwrap().len(), 1);
}

#[test]
fn only_a_far_jump_across_missing_ticks_warns() {
    let near = [line(10, 0, 0.0, 0.0), line(14, 0, 60.0, 0.0)];
    assert!(check(&near, &limits(&[])).unwrap().warnings.is_empty());
    let one_tick = [line(10, 0, 0.0, 0.0), line(11, 0, 70.0, 0.0)];
    let failed = check(&one_tick, &limits(&[])).unwrap();
    assert!(!failed.passed);
    assert!(failed.warnings.is_empty());
    let over_time = [line(10, 0, 0.0, 0.0), line(12, 0, 200.0, 0.0)];
    let failed = check(&over_time, &limits(&[])).unwrap();
    assert!(!failed.passed);
    assert!(failed.warnings.is_empty());
}

#[test]
fn the_contact_sheet_of_a_stepped_run_is_no_frame() {
    let dir = std::env::temp_dir().join(format!("voidc-sheet-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let frames: Vec<Frame> = (0..2).map(|i| line(10, i, 0.0, 0.0)).collect();
    for name in ["f00020.png", "f00021.png", "sheet.png"] {
        Picture {
            width: 1,
            height: 1,
            rgb: vec![0; 3],
        }
        .save(&dir.join(name))
        .unwrap();
    }
    let images = format!("images={}", dir.display());
    let outcome = check(&frames, &limits(&[&images])).unwrap();
    assert_eq!(outcome.pictures, 2);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn what_was_not_checked_is_said_on_a_clean_run_too() {
    let outcome = check(&smooth(), &limits(&[])).unwrap();
    assert!(outcome.passed);
    assert!(outcome.summary().contains("not checked: flashes"));
}

#[test]
fn a_broken_file_is_an_error_with_its_line() {
    assert!(parse_frames("").is_err());
    assert!(parse_frames("1 2\n").unwrap_err().contains("line 1"));
    assert!(
        parse_frames("{\"t\":1,\"x\":1.0,\"z\":0.0}\nnope\n")
            .unwrap_err()
            .contains("line 2")
    );
    let backwards = [line(5, 0, 0.0, 0.0), line(4, 0, 0.0, 0.0)];
    assert!(check(&backwards, &limits(&[])).is_err());
}

#[test]
fn a_flash_needs_pictures_and_is_found_by_tick_and_image() {
    let dir = std::env::temp_dir().join(format!("voidc-flash-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let frames = [
        line(7, 0, 0.0, 0.0),
        line(7, 1, 0.0, 0.0),
        line(7, 2, 0.0, 0.0),
    ];
    for (image, level) in [(0, 120u8), (1, 118), (2, 60)] {
        Picture {
            width: 2,
            height: 2,
            rgb: vec![level; 12],
        }
        .save(&dir.join(format!("{}-{image}.png", 7)))
        .unwrap();
    }
    let images = format!("images={}", dir.display());
    let outcome = check(&frames, &limits(&[&images])).unwrap();
    assert_eq!(outcome.pictures, 3);
    assert_eq!(outcome.findings.len(), 1);
    assert_eq!(
        (outcome.findings[0].kind, outcome.findings[0].k),
        ("flash", 2)
    );
    assert!(
        check(&frames, &limits(&[&images, "cut=7:2"]))
            .unwrap()
            .passed
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn pictures_of_a_stepped_run_are_found_by_their_index() {
    let dir = std::env::temp_dir().join(format!("voidc-index-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let frames: Vec<Frame> = (0..8)
        .map(|i| line(10 + i / 4, (i % 4) as u32, 0.0, 0.0))
        .collect();
    for (index, level) in [(40, 120u8), (44, 40)] {
        Picture {
            width: 2,
            height: 2,
            rgb: vec![level; 12],
        }
        .save(&dir.join(format!("f{index:05}.png")))
        .unwrap();
    }
    let images = format!("images={}", dir.display());
    let outcome = check(&frames, &limits(&[&images])).unwrap();
    assert_eq!(outcome.pictures, 2);
    let finding = &outcome.findings[0];
    assert_eq!((finding.kind, finding.t, finding.k), ("flash", 11, 0));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn only_names_of_a_stepped_run_are_read_as_an_index() {
    assert_eq!(picture_index("f00241"), Some(241));
    assert_eq!(picture_index("f1234567"), Some(1234567));
    for name in ["shot40", "40", "f41", "f0004x", "frame00040", "F00040"] {
        assert_eq!(picture_index(name), None, "{name}");
    }
    let dir = std::env::temp_dir().join(format!("voidc-not-index-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let frames: Vec<Frame> = (0..8).map(|i| line(10, i as u32, 0.0, 0.0)).collect();
    for name in ["shot80", "shot84"] {
        Picture {
            width: 1,
            height: 1,
            rgb: vec![0; 3],
        }
        .save(&dir.join(format!("{name}.png")))
        .unwrap();
    }
    let images = format!("images={}", dir.display());
    let error = check(&frames, &limits(&[&images])).unwrap_err();
    assert!(error.contains("2 pictures for 8 frames"), "{error}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn pictures_that_match_no_frame_are_refused() {
    let dir = std::env::temp_dir().join(format!("voidc-pictures-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    Picture {
        width: 1,
        height: 1,
        rgb: vec![0; 3],
    }
    .save(&dir.join("shot.png"))
    .unwrap();
    let images = format!("images={}", dir.display());
    assert!(check(&smooth(), &limits(&[&images])).is_err());
    fs::remove_dir_all(dir).unwrap();
}
