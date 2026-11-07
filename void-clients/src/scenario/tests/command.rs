use super::*;

#[test]
fn a_client_that_cannot_be_opened_is_still_stopped() {
    let dir = staged("no-open", "act map\n");
    let mut stage = Stage::new(&dir);
    stage.driver = Err("client carte does not answer".into());
    let error = command(&words(&dir, &["--feature", "Carte"]), &mut stage).unwrap_err();
    assert!(error.contains("does not answer"), "{error}");
    assert_eq!(stage.downs, ["carte"]);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_run_starts_its_client_stops_it_and_writes_its_report() {
    let dir = staged("command", "act map\n");
    let mut stage = Stage::new(&dir);
    command(&words(&dir, &["--feature", "Carte"]), &mut stage).unwrap();
    assert_eq!((stage.ups.len(), stage.downs.len()), (1, 1));
    assert!(dir.join("out/report.json").exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn keep_up_leaves_the_client_and_a_client_of_someone_else_is_never_stopped() {
    let dir = staged("keep-up", "act map\n");
    let mut stage = Stage::new(&dir);
    command(
        &words(&dir, &["--feature", "Carte", "--keep-up"]),
        &mut stage,
    )
    .unwrap();
    assert!(stage.downs.is_empty());
    stage.clients.insert("carte".into(), (Level::Render, true));
    command(&words(&dir, &["--on", "carte"]), &mut stage).unwrap();
    assert!(stage.downs.is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_client_that_never_gets_in_game_does_not_stay_alive() {
    let dir = staged("no-join", "act map\n");
    let mut stage = Stage::new(&dir);
    stage.up = Err("not in game after 90 s".into());
    let error = command(&words(&dir, &["--feature", "Carte"]), &mut stage).unwrap_err();
    assert!(error.contains("not in game"));
    assert_eq!(stage.downs, ["carte"]);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_failed_stop_does_not_lose_the_report() {
    let dir = staged("no-stop", "act map\n");
    let mut stage = Stage::new(&dir);
    stage.down = Err("carte did not stop".into());
    let error = command(&words(&dir, &["--feature", "Carte"]), &mut stage).unwrap_err();
    assert!(error.contains("did not stop"));
    assert!(dir.join("out/report.txt").exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_driver_that_fails_keeps_the_failed_stop_that_follows() {
    let dir = staged("no-driver-no-stop", "act map\n");
    let mut stage = Stage::new(&dir);
    stage.driver = Err("client carte does not answer".into());
    stage.down = Err("carte did not stop".into());
    let error = command(&words(&dir, &["--feature", "Carte"]), &mut stage).unwrap_err();
    assert!(error.contains("does not answer"), "{error}");
    assert!(error.contains("did not stop"), "{error}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_preflight_that_refuses_keeps_the_failed_stop_that_follows() {
    let dir = staged("no-preflight-no-stop", "wait_until chunkz>=1\n");
    let mut stage = Stage::new(&dir);
    stage.down = Err("carte did not stop".into());
    let error = command(&words(&dir, &["--feature", "Carte"]), &mut stage).unwrap_err();
    assert!(error.contains("no field chunkz"), "{error}");
    assert!(error.contains("did not stop"), "{error}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_report_that_cannot_be_written_keeps_the_failed_stop_that_follows() {
    let dir = staged("no-report-no-stop", "act map\n");
    fs::create_dir_all(dir.join("out/report.txt")).unwrap();
    let mut stage = Stage::new(&dir);
    stage.down = Err("carte did not stop".into());
    let error = command(&words(&dir, &["--feature", "Carte"]), &mut stage).unwrap_err();
    assert!(error.contains("Is a directory"), "{error}");
    assert!(error.contains("did not stop"), "{error}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_failed_line_exits_with_an_error_and_stops_the_client() {
    let dir = staged("failed-line", "frames 500\n");
    let mut stage = Stage::new(&dir);
    let error = command(&words(&dir, &["--feature", "Carte"]), &mut stage).unwrap_err();
    assert!(error.is_empty(), "{error}");
    assert_eq!(stage.downs, ["carte"]);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_refused_scenario_starts_no_client() {
    let dir = staged("refused", "level bot\nshot a\n");
    let mut stage = Stage::new(&dir);
    assert!(command(&words(&dir, &["--feature", "Carte"]), &mut stage).is_err());
    assert!(stage.ups.is_empty() && stage.downs.is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_target_is_a_running_client_of_the_right_level() {
    let dir = folder("target");
    let mut stage = Stage::new(&dir);
    stage.clients.insert("carte".into(), (Level::Render, true));
    stage.clients.insert("mort".into(), (Level::Render, false));
    stage.clients.insert("jouer".into(), (Level::Play, true));
    let args =
        |line: &str| -> Vec<String> { line.split_whitespace().map(str::to_string).collect() };
    let plain = Scenario::parse("act map\n").unwrap();
    let bot = Scenario::parse("level bot\nact map\n").unwrap();
    let on = target(&args("--on carte"), &plain, &stage).unwrap();
    assert_eq!((on.level, on.feature), (Level::Render, None));
    assert!(target(&args("--on nul"), &plain, &stage).is_err());
    assert!(target(&args("--on mort"), &plain, &stage).is_err());
    assert!(target(&args("--on jouer"), &plain, &stage).is_err());
    assert!(target(&args("--on carte"), &bot, &stage).is_err());
    assert!(target(&args("--on carte --level logic"), &plain, &stage).is_err());
    assert!(target(&[], &plain, &stage).is_err());
    let error = target(&args("--feature Carte"), &plain, &stage)
        .err()
        .unwrap();
    assert!(error.contains("--on carte"), "{error}");
    let fresh = target(&args("--feature Neuve --level logic"), &plain, &stage).unwrap();
    assert_eq!((fresh.name.as_str(), fresh.level), ("neuve", Level::Logic));
    assert!(target(&args("--feature Neuve --level play"), &plain, &stage).is_err());
    fs::remove_dir_all(dir).unwrap();
}
