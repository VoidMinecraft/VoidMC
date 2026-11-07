use super::*;

#[test]
fn a_condition_is_sent_as_written_and_a_wait_carries_its_limit() {
    let scenario = Scenario::parse(
        "wait_until chunks >= 200\nwait_until 30000 chunks >= 200\nassert chat~\"Bienvenue sur  la carte\"\n",
    )
    .unwrap();
    let sent: Vec<String> = scenario
        .lines
        .iter()
        .map(|line| {
            let (condition, limit) = line.condition.as_ref().unwrap();
            verb_of(&line.verb, condition, *limit)
        })
        .collect();
    assert_eq!(
        sent,
        [
            format!("wait_until {DEFAULT_WAIT_MS} chunks >= 200"),
            "wait_until 30000 chunks >= 200".into(),
            "assert chat~\"Bienvenue sur  la carte\"".into(),
        ]
    );
    assert!(Scenario::parse("assert chunks\n").is_err());
    assert!(Scenario::parse("assert >= 3\n").is_err());
    assert!(Scenario::parse("assert chat~Bienvenue sur la carte\n").is_err());
    assert!(Scenario::parse("wait_until chat~Tour 2\n").is_err());
}

#[test]
fn a_quoted_condition_reaches_the_probe_intact_and_reads_the_same() {
    let scenario = Scenario::parse("wait_until 2 action_bar~\"Tour 2\"\n").unwrap();
    let line = &scenario.lines[0];
    let (written, limit) = line.condition.as_ref().unwrap();
    let sent = verb_of(&line.verb, written, *limit);
    assert_eq!(sent, "wait_until 2 action_bar~\"Tour 2\"");
    let (verb, rest) = condition::split(&sent);
    let (reread, reread_limit) = condition::of(verb, rest).unwrap().unwrap();
    assert_eq!((&reread, reread_limit), (&written.parsed, 2));
    let row = format!("wait\trender\t{rest}\t2 false");
    assert!(
        include_str!("../../../conditions.tsv")
            .lines()
            .any(|case| case == row),
        "the probe reads {rest:?} through conditions.tsv: {row:?}"
    );
}

#[test]
fn comments_and_blank_lines_keep_the_numbers_of_the_file() {
    let scenario = Scenario::parse("# top\n\nlevel bot\nact map\n").unwrap();
    assert_eq!(scenario.level, Some(Level::Bot));
    assert_eq!(scenario.lines[0].number, 4);
}

#[test]
fn a_verb_of_another_level_is_refused_with_every_line_at_fault() {
    let text = "act map\nshot a\ncompare a a max=1\nturn 1 2\nfly\n";
    let scenario = Scenario::parse(text).unwrap();
    let error = scenario.validate(Level::Bot, Path::new(".")).unwrap_err();
    assert!(error.contains("line 2: shot does not exist at level bot, only at render"));
    assert!(error.contains("line 3: compare"));
    assert!(error.contains("line 4: turn"));
    assert!(error.contains("line 5: unknown verb fly"));
    assert!(!error.contains("line 1"));
    assert!(
        scenario
            .validate(Level::Render, Path::new("."))
            .unwrap_err()
            .contains("fly")
    );
}

#[test]
fn the_stepping_verbs_run_on_a_game_and_not_on_a_bot() {
    let scenario = Scenario::parse("clock virtual\nstep 4\nfence 10\nsent\n").unwrap();
    scenario.validate(Level::Render, Path::new(".")).unwrap();
    assert!(scenario.validate(Level::Bot, Path::new(".")).is_err());
}

#[test]
fn a_clock_takes_virtual_or_real_and_nothing_else() {
    let scenario = Scenario::parse("clock lock\n").unwrap();
    let error = scenario
        .validate(Level::Render, Path::new("."))
        .unwrap_err();
    assert!(error.contains("virtual or real"), "{error}");
    let scenario = Scenario::parse("clock real\n").unwrap();
    scenario.validate(Level::Render, Path::new(".")).unwrap();
}

#[test]
fn only_a_virtual_clock_is_refused_while_a_recording_runs() {
    for text in [
        "frames 500\nclock virtual\n",
        "clock virtual\nframes 300\nstep 50 100\nclock real\nclock virtual\n",
        "track 300\nclock virtual\n",
    ] {
        let error = Scenario::parse(text)
            .unwrap()
            .validate(Level::Render, Path::new("."))
            .unwrap_err();
        assert!(error.contains("still recording"), "{text}: {error}");
    }
    for allowed in [
        "clock virtual\nframes 300\nstep 50 100\ncheck frames\nclock real\n",
        "clock virtual\nframes 300\nstep 50 100\nclock real\n",
        "track 300\nclock real\n",
    ] {
        Scenario::parse(allowed)
            .unwrap()
            .validate(Level::Render, Path::new("."))
            .unwrap();
    }
}

#[test]
fn the_stepping_verbs_take_the_words_of_the_probe() {
    for (verb, least) in [("clock", 1), ("step", 1), ("fence", 1), ("sent", 0)] {
        let entry = VERBS.iter().find(|entry| entry.0 == verb).unwrap();
        assert_eq!(entry.2, least, "{verb}");
    }
}

#[test]
fn every_verb_forwarded_to_a_game_exists_in_its_probe() {
    let local = ["check", "compare", "wait_still"];
    for (verb, levels, _) in VERBS {
        let forwarded = levels.contains(&Level::Render) && !local.contains(verb);
        if forwarded {
            assert!(crate::launch::probe_answers(verb), "{verb}");
        }
    }
}

#[test]
fn the_scenario_of_the_repository_is_valid() {
    let text = include_str!("../../../scenarios/open-map.scn");
    let scenario = Scenario::parse(text).unwrap();
    assert_eq!(scenario.level, Some(Level::Render));
    scenario.validate(Level::Render, Path::new(".")).unwrap();
    assert!(scenario.validate(Level::Logic, Path::new(".")).is_err());
}

#[test]
fn a_scenario_with_nothing_to_play_is_refused_and_never_passes() {
    let scenario = Scenario::parse("# only a comment\nlevel render\n").unwrap();
    assert!(scenario.validate(Level::Render, Path::new(".")).is_err());
    let dir = folder("empty");
    let mut fake = Fake::new(&dir, vec![]);
    assert!(!run(&dir, false).play(&scenario, &mut fake).unwrap().passed);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_number_operator_on_a_text_is_refused_on_every_level() {
    for level in [Level::Bot, Level::Logic, Level::Render] {
        let scenario = Scenario::parse("assert chat >= 5\n").unwrap();
        let error = scenario.validate(level, Path::new(".")).unwrap_err();
        assert!(error.contains("line 1"), "{error}");
    }
    let camera = Scenario::parse("assert camera >= 3\n").unwrap();
    assert!(camera.validate(Level::Render, Path::new(".")).is_err());
    assert!(camera.validate(Level::Bot, Path::new(".")).is_ok());
    let word = Scenario::parse("assert chunks >= many\n").unwrap_err();
    assert!(word.contains("line 1"), "{word}");
    assert!(Scenario::parse("assert chunks >= 1e3\n").is_err());
}

#[test]
fn bad_numbers_of_a_recording_or_a_wait_are_refused() {
    for text in [
        "frames lots\n",
        "frames 0\n",
        "track x\n",
        "wait_still soon\n",
    ] {
        let scenario = Scenario::parse(text).unwrap();
        assert!(
            scenario.validate(Level::Render, Path::new(".")).is_err(),
            "{text}"
        );
    }
}

#[test]
fn a_check_on_a_file_that_does_not_exist_is_refused() {
    let scenario = Scenario::parse("check nothing.jsonl\n").unwrap();
    assert!(scenario.validate(Level::Render, Path::new(".")).is_err());
}

#[test]
fn a_wait_keeps_its_limit_and_an_assert_has_none() {
    let scenario =
        Scenario::parse("wait_until 12 chat == Score\nassert chat == \"Score 12\"\n").unwrap();
    let (wait, limit) = scenario.lines[0].condition.as_ref().unwrap();
    assert_eq!((wait.text.as_str(), *limit), ("chat == Score", 12));
    let (assertion, _) = scenario.lines[1].condition.as_ref().unwrap();
    assert_eq!(assertion.text, "chat == \"Score 12\"");
    let scenario = Scenario::parse("wait_until chat~\"Tour 2\"\n").unwrap();
    let (wait, limit) = scenario.lines[0].condition.as_ref().unwrap();
    assert_eq!(
        (wait.text.as_str(), *limit),
        ("chat~\"Tour 2\"", DEFAULT_WAIT_MS)
    );
}

#[test]
fn a_comparison_must_name_real_pictures_and_a_limit() {
    let scenario = Scenario::parse("compare before after max=1\n").unwrap();
    let error = scenario
        .validate(Level::Render, Path::new("."))
        .unwrap_err();
    assert!(error.contains("before is neither"));
    let scenario = Scenario::parse("shot a\ncompare a a\n").unwrap();
    let error = scenario
        .validate(Level::Render, Path::new("."))
        .unwrap_err();
    assert!(error.contains("nothing to hold"));
}

#[test]
fn a_shot_after_its_comparison_is_too_late() {
    let scenario = Scenario::parse("compare a a max=1\nshot a\n").unwrap();
    assert!(scenario.validate(Level::Render, Path::new(".")).is_err());
}

#[test]
fn an_assert_keeps_its_last_word() {
    let dir = folder("assert-word");
    let scenario = Scenario::parse("assert chat == \"Score 12\"\n").unwrap();
    let mut fake = Fake::new(&dir, vec![]);
    fake.state["chat"] = json!("Score 12");
    run(&dir, false).play(&scenario, &mut fake).unwrap();
    assert_eq!(fake.asked, ["assert chat == \"Score 12\"", "clock real"]);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_condition_on_the_command_line_is_refused_as_in_a_scenario() {
    let refused_by_scenario = |verb: &str, level: Level| {
        Scenario::parse(verb)
            .and_then(|scenario| scenario.validate(level, Path::new(".")))
            .unwrap_err()
    };
    let one_word = "a value is one word";
    for (verb, error) in [
        ("assert chunks >= 200 30000", one_word),
        ("wait_until chunks >= 200 30s", one_word),
        ("assert chat >= 3", "chat is not a number"),
        ("assert screen==", "compares with nothing"),
        ("wait_until chunks >= 200 30000", one_word),
        ("wait_until 30s chunks >= 200", "bad condition 30s chunks"),
    ] {
        for level in ALL {
            let refused = condition::check(verb, *level).unwrap_err();
            assert!(refused.contains(error), "{verb}: {refused}");
            let refused = refused_by_scenario(verb, *level);
            assert!(refused.contains(error), "{verb}: {refused}");
        }
    }
    let limit = Limits::REAL.of("wait_until chunks >= 200");
    assert_eq!(limit, Duration::from_millis(DEFAULT_WAIT_MS + 5_000));
}

#[test]
fn what_a_scenario_does_not_play_is_refused_and_the_readme_says_so() {
    let readme = include_str!("../../../README.md");
    for verb in ["show", "hide", "thumb", "perf", "quit"] {
        let scenario = Scenario::parse(verb).unwrap();
        let error = scenario
            .validate(Level::Render, Path::new("."))
            .unwrap_err();
        assert!(error.contains("unknown verb"), "{error}");
        assert!(readme.contains(&format!("`{verb}`")), "{verb}");
    }
    assert!(!readme.contains("Every verb of the level is there"));
}
