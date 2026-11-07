use super::*;

#[test]
fn a_new_server_takes_the_client_through_a_configuration() {
    let mut rig = Rig::in_play();
    rig.lose_backend();
    rig.tick(settings().retry_every);
    rig.clear();
    rig.connected();
    assert!(rig.client.is_empty());
    rig.receive_from_backend(login_finished());
    assert_eq!(rig.client, vec![empty(play_cb::START_CONFIGURATION)]);
    assert!(rig.backend.contains(&empty(login_sb::LOGIN_ACKNOWLEDGED)));
    assert!(rig.backend.contains(&information()));
    rig.clear();

    rig.receive_from_backend(known_packs_question());
    rig.receive_from_backend(registry(2));
    assert!(rig.client.is_empty());
    rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
    assert!(rig.backend.is_empty());
    assert_eq!(rig.client, vec![known_packs_question(), registry(2)]);
    rig.receive_from_client(known_packs_reply());
    assert_eq!(rig.backend, vec![known_packs_reply()]);

    rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
    assert_eq!(rig.resumed(), 0);
    rig.receive_from_client(empty(config_sb::FINISH_CONFIGURATION));
    assert_eq!(rig.resumed(), 1);
    rig.receive_from_backend(login());
    rig.receive_from_client(empty(play_sb::PLAYER_LOADED));
    assert_eq!(rig.sent_to_backend(play_sb::PLAYER_LOADED), 1);
    let commands = frame::build(play_cb::COMMANDS, |_| {});
    rig.receive_from_backend(commands.clone());
    assert_eq!(rig.client.last(), Some(&commands));
}

#[test]
fn keep_alives_of_a_configuration_are_answered_and_do_not_wait_for_the_client() {
    let mut rig = Rig::in_play();
    rig.lose_backend();
    rig.log_back_in();
    rig.clear();
    rig.receive_from_backend(frame::build(config_cb::KEEP_ALIVE, |body| {
        9i64.encode(body)
    }));
    rig.receive_from_backend(frame::build(config_cb::PING, |body| 4i32.encode(body)));
    assert_eq!(rig.sent_to_backend(config_sb::KEEP_ALIVE), 1);
    assert_eq!(rig.sent_to_backend(config_sb::PONG), 1);
    rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
    assert!(rig.client.is_empty());
}

#[test]
fn every_return_goes_through_a_configuration() {
    let mut rig = Rig::in_play();
    for _ in 0..3 {
        rig.lose_backend();
        rig.come_back();
        assert_eq!(rig.sent_to_client(play_cb::START_CONFIGURATION), 1);
        assert_eq!(rig.resumed(), 1);
        rig.receive_from_client(empty(play_sb::PLAYER_LOADED));
        rig.clear();
    }
}

#[test]
fn frames_held_for_a_slow_client_arrive_once_it_acknowledges_after_the_login() {
    let mut rig = Rig::in_play();
    rig.lose_backend();
    rig.log_back_in();
    rig.receive_from_backend(known_packs_question());
    assert_eq!(rig.sent_to_client(config_cb::SELECT_KNOWN_PACKS), 0);
    rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
    assert_eq!(rig.sent_to_client(config_cb::SELECT_KNOWN_PACKS), 1);
    rig.receive_from_client(known_packs_reply());
    assert_eq!(rig.sent_to_backend(config_sb::SELECT_KNOWN_PACKS), 1);
}

#[test]
fn a_client_that_acknowledges_before_the_login_is_relayed_live() {
    let mut rig = Rig::in_play();
    rig.lose_backend();
    rig.tick(settings().retry_every);
    rig.connected();
    rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
    rig.receive_from_backend(login_finished());
    rig.clear();
    rig.receive_from_backend(known_packs_question());
    assert_eq!(rig.client, vec![known_packs_question()]);
    rig.receive_from_client(known_packs_reply());
    assert_eq!(rig.backend, vec![known_packs_reply()]);
}

#[test]
fn no_server_at_all_ends_with_a_login_disconnect() {
    let mut rig = Rig::fresh();
    rig.receive_from_client(hello());
    rig.tick(Duration::from_secs(11));
    assert_eq!(rig.sent_to_client(login_cb::LOGIN_DISCONNECT), 1);
}

#[test]
fn a_server_lost_during_the_configuration_ends_with_a_configuration_disconnect() {
    let mut rig = Rig::fresh();
    rig.tick(Duration::ZERO);
    rig.connected();
    rig.receive_from_client(hello());
    rig.receive_from_backend(login_finished());
    rig.receive_from_client(empty(login_sb::LOGIN_ACKNOWLEDGED));
    rig.clear();
    rig.lose_backend();
    rig.tick(Duration::from_millis(100));
    assert_eq!(rig.sent_to_client(config_cb::KEEP_ALIVE), 1);
    rig.tick(Duration::from_secs(11));
    assert_eq!(rig.sent_to_client(config_cb::DISCONNECT), 1);
}

#[test]
fn a_client_in_configuration_goes_on_with_the_new_server_live() {
    let mut rig = Rig::fresh();
    rig.tick(Duration::ZERO);
    rig.connected();
    rig.receive_from_client(hello());
    rig.receive_from_backend(login_finished());
    rig.receive_from_client(empty(login_sb::LOGIN_ACKNOWLEDGED));
    rig.receive_from_client(information());
    rig.lose_backend();
    rig.log_back_in();
    rig.clear();
    rig.receive_from_backend(known_packs_question());
    assert_eq!(rig.client, vec![known_packs_question()]);
    rig.receive_from_client(known_packs_reply());
    assert_eq!(rig.backend, vec![known_packs_reply()]);
}

#[test]
fn a_client_that_never_acknowledges_its_login_gets_a_configuration_disconnect() {
    let mut rig = Rig::fresh();
    rig.tick(Duration::ZERO);
    rig.connected();
    rig.receive_from_client(hello());
    rig.receive_from_backend(login_finished());
    rig.clear();
    rig.lose_backend();
    rig.tick(Duration::from_secs(11));
    assert_eq!(rig.sent_to_client(config_cb::DISCONNECT), 1);
    assert_eq!(rig.sent_to_client(login_cb::LOGIN_DISCONNECT), 0);
}

#[test]
fn a_client_on_its_way_to_a_configuration_is_disconnected_in_configuration() {
    let mut rig = Rig::in_play();
    rig.receive_from_backend(empty(play_cb::START_CONFIGURATION));
    rig.clear();
    rig.receive_from_backend(frame::build(play_cb::SYSTEM_CHAT, |body| {
        body.resize(MAX_PLAIN_FRAME_BYTES + 1, 0);
    }));
    assert_eq!(rig.sent_to_client(config_cb::DISCONNECT), 1);
    assert_eq!(rig.sent_to_client(play_cb::DISCONNECT), 0);
}

#[test]
fn a_client_told_to_reconfigure_is_disconnected_in_configuration() {
    let mut rig = Rig::in_play();
    rig.lose_backend();
    rig.tick(settings().retry_every);
    rig.connected();
    rig.receive_from_backend(login_finished());
    rig.clear();
    rig.tick(Duration::from_secs(11));
    rig.tick(Duration::ZERO);
    assert_eq!(rig.sent_to_client(config_cb::DISCONNECT), 1);
    assert_eq!(rig.sent_to_client(play_cb::DISCONNECT), 0);
}

#[test]
fn a_client_on_its_way_to_play_reads_nothing_of_a_configuration() {
    let mut rig = Rig::in_configuration();
    rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
    rig.clear();
    rig.lose_backend();
    rig.tick(Duration::ZERO);
    assert_eq!(rig.sent_to_client(config_cb::KEEP_ALIVE), 0);
    rig.tick(Duration::from_secs(11));
    assert_eq!(rig.sent_to_client(play_cb::DISCONNECT), 1);
    assert_eq!(rig.sent_to_client(config_cb::DISCONNECT), 0);
}

#[test]
fn a_new_server_does_not_repeat_what_a_configuring_client_received() {
    let mut rig = Rig::in_configuration();
    rig.lose_backend();
    rig.log_back_in();
    rig.clear();

    rig.receive_from_backend(known_packs_question());
    assert_eq!(rig.backend, vec![known_packs_reply()]);
    rig.receive_from_backend(registry(1));
    assert!(rig.client.is_empty());

    rig.receive_from_backend(registry(2));
    assert_eq!(rig.client, vec![registry(2)]);
    rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
    rig.receive_from_client(empty(config_sb::FINISH_CONFIGURATION));
    assert_eq!(rig.sent_to_backend(config_sb::FINISH_CONFIGURATION), 1);
    assert_eq!(rig.resumed(), 1);
}

#[test]
fn a_server_that_differs_from_what_a_configuring_client_holds_ends_the_session() {
    let mut rig = Rig::in_configuration();
    rig.lose_backend();
    rig.log_back_in();
    rig.clear();
    rig.receive_from_backend(known_packs_question());
    rig.receive_from_backend(registry(9));
    assert_eq!(rig.sent_to_client(config_cb::REGISTRY_DATA), 0);
    assert_eq!(rig.sent_to_client(config_cb::DISCONNECT), 1);
    assert!(rig.rest.contains(&Action::DropBackend));
    assert!(rig.rest.contains(&Action::Close));
    assert!(matches!(rig.events()[..], [Event::GaveUp { .. }]));
}

#[test]
fn a_question_the_client_has_not_answered_yet_waits_for_its_reply() {
    let mut rig = Rig::fresh();
    rig.tick(Duration::ZERO);
    rig.connected();
    rig.receive_from_client(hello());
    rig.receive_from_backend(login_finished());
    rig.receive_from_client(empty(login_sb::LOGIN_ACKNOWLEDGED));
    rig.receive_from_backend(known_packs_question());
    rig.receive_from_backend(registry(1));
    rig.lose_backend();
    rig.log_back_in();
    rig.clear();

    rig.receive_from_backend(known_packs_question());
    assert_eq!(rig.sent_to_backend(config_sb::SELECT_KNOWN_PACKS), 0);
    rig.receive_from_client(known_packs_reply());
    assert_eq!(rig.backend, vec![known_packs_reply()]);
    rig.receive_from_backend(registry(1));
    assert!(rig.client.is_empty());
}

#[test]
fn a_client_that_ends_its_configuration_during_a_replay_goes_through_a_new_one() {
    let mut rig = Rig::in_configuration();
    rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
    rig.lose_backend();
    rig.log_back_in();
    rig.clear();
    rig.receive_from_backend(known_packs_question());
    rig.receive_from_client(empty(config_sb::FINISH_CONFIGURATION));
    assert!(rig.rest.contains(&Action::DropBackend));
    assert_eq!(rig.sent_to_backend(config_sb::FINISH_CONFIGURATION), 0);
    rig.tick(Duration::ZERO);
    rig.connected();
    rig.receive_from_backend(login_finished());
    assert_eq!(rig.sent_to_client(play_cb::START_CONFIGURATION), 1);
}

#[test]
fn a_server_lost_during_the_login_ends_in_resumed_and_the_next_crash_is_seen() {
    let mut rig = Rig::fresh();
    rig.tick(Duration::ZERO);
    rig.connected();
    rig.receive_from_client(hello());
    rig.lose_backend();
    rig.tick(settings().retry_every);
    rig.connected();
    rig.receive_from_backend(login_finished());
    rig.receive_from_client(empty(login_sb::LOGIN_ACKNOWLEDGED));
    rig.receive_from_backend(known_packs_question());
    rig.receive_from_client(known_packs_reply());
    rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
    rig.receive_from_client(empty(config_sb::FINISH_CONFIGURATION));
    assert_eq!(rig.resumed(), 1);
    rig.receive_from_backend(login());
    rig.clear();

    rig.lose_backend();
    assert_eq!(rig.events(), vec![&Event::BackendLost]);
}

#[test]
fn a_server_that_freezes_once_the_client_acknowledged_is_tried_again() {
    let mut rig = Rig::in_play();
    rig.lose_backend();
    rig.log_back_in();
    rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
    rig.receive_from_backend(known_packs_question());
    rig.clear();
    rig.tick(settings().backend_silence - Duration::from_secs(1));
    assert!(!rig.rest.contains(&Action::DropBackend));
    rig.tick(Duration::from_secs(1));
    assert!(rig.rest.contains(&Action::DropBackend));
    assert!(rig.events().is_empty());
    rig.tick(settings().retry_every);
    assert!(rig.rest.contains(&Action::Connect));
    assert_eq!(rig.sent_to_client(config_cb::KEEP_ALIVE), 1);
}

#[test]
fn a_client_that_reaches_its_configuration_waits_the_whole_return_timeout() {
    let patient = Settings {
        return_timeout: Duration::from_secs(60),
        ..settings()
    };
    let mut rig = Rig::with(patient);
    rig.tick(Duration::ZERO);
    rig.connected();
    rig.receive_from_client(hello());
    rig.receive_from_backend(login_finished());
    rig.lose_backend();
    rig.receive_from_client(empty(login_sb::LOGIN_ACKNOWLEDGED));
    rig.clear();
    rig.tick(Duration::from_secs(30));
    assert_eq!(rig.sent_to_client(config_cb::DISCONNECT), 0);
    rig.tick(Duration::from_secs(31));
    assert_eq!(rig.sent_to_client(config_cb::DISCONNECT), 1);
}

#[test]
fn a_question_only_the_client_answers_ends_a_replay_with_a_message() {
    let cookie = frame::build(config_cb::COOKIE_REQUEST, |body| {
        "minecraft:token".to_string().encode(body);
    });
    let mut rig = Rig::in_configuration();
    rig.receive_from_backend(cookie.clone());
    rig.lose_backend();
    rig.log_back_in();
    rig.clear();

    rig.receive_from_backend(known_packs_question());
    rig.receive_from_backend(registry(1));
    rig.receive_from_backend(cookie);
    assert_eq!(rig.sent_to_client(config_cb::DISCONNECT), 1);
    assert!(rig.rest.contains(&Action::DropBackend));
    assert!(matches!(
        rig.events()[..],
        [Event::GaveUp { reason }] if reason.contains("cookie")
    ));
}

#[test]
fn nothing_of_play_reaches_a_client_on_its_way_to_a_configuration() {
    let pack = Uuid::from_u128(5);
    let mut rig = Rig::in_play();
    rig.receive_from_backend(push(pack, "abc"));
    rig.receive_from_client(pack_answer(pack, 0));
    rig.lose_backend();
    rig.come_back();
    rig.receive_from_backend(empty(play_cb::START_CONFIGURATION));
    rig.clear();

    rig.tick(settings().settle);
    assert_eq!(rig.sent_to_client(play_cb::RESOURCE_PACK_POP), 0);
    rig.lose_backend();
    rig.tick(Duration::ZERO);
    assert_eq!(rig.sent_to_client(play_cb::KEEP_ALIVE), 0);
    assert_eq!(rig.sent_to_client(play_cb::SET_ACTION_BAR_TEXT), 0);
}

#[test]
fn a_client_acknowledging_the_end_of_configuration_as_the_server_dies_still_returns() {
    let mut rig = Rig::fresh();
    rig.tick(Duration::ZERO);
    rig.connected();
    rig.receive_from_client(hello());
    rig.receive_from_backend(login_finished());
    rig.receive_from_client(empty(login_sb::LOGIN_ACKNOWLEDGED));
    rig.receive_from_backend(known_packs_question());
    rig.receive_from_client(known_packs_reply());
    rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
    rig.lose_backend();
    rig.receive_from_client(empty(config_sb::FINISH_CONFIGURATION));
    rig.clear();
    rig.tick(settings().retry_every);
    rig.connected();
    rig.receive_from_backend(login_finished());
    assert_eq!(rig.sent_to_client(play_cb::START_CONFIGURATION), 1);
    rig.receive_from_backend(known_packs_question());
    rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
    assert!(!rig.rest.contains(&Action::DropBackend));
    rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
    assert_eq!(rig.sent_to_client(config_cb::FINISH_CONFIGURATION), 1);
    rig.receive_from_client(known_packs_reply());
    rig.receive_from_client(empty(config_sb::FINISH_CONFIGURATION));
    assert_eq!(rig.resumed(), 1);
}

#[test]
fn the_away_message_is_cleared_right_after_the_login() {
    let mut rig = Rig::in_play();
    rig.lose_backend();
    rig.tick(Duration::ZERO);
    rig.log_back_in();
    rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
    rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
    rig.receive_from_client(empty(config_sb::FINISH_CONFIGURATION));
    rig.clear();
    rig.receive_from_backend(login());
    assert_eq!(rig.client, vec![login(), action_bar("")]);
}

#[test]
fn a_server_in_online_mode_is_refused_with_a_reason() {
    let mut rig = Rig::fresh();
    rig.tick(Duration::ZERO);
    rig.connected();
    rig.receive_from_backend(empty(login_cb::HELLO));
    assert_eq!(rig.sent_to_client(login_cb::LOGIN_DISCONNECT), 1);
    assert!(rig.rest.contains(&Action::Close));
}

#[test]
fn a_login_question_only_a_client_could_answer_is_answered_with_nothing() {
    let mut rig = Rig::in_play();
    rig.lose_backend();
    rig.tick(settings().retry_every);
    rig.connected();
    rig.receive_from_backend(frame::build(login_cb::CUSTOM_QUERY, |body| {
        VarI32(3).encode(body);
    }));
    assert_eq!(rig.sent_to_backend(login_sb::CUSTOM_QUERY_ANSWER), 1);
}

#[test]
fn a_server_lost_after_ending_the_configuration_is_replayed_to_its_end() {
    let mut rig = Rig::in_configuration();
    rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
    rig.lose_backend();
    rig.log_back_in();
    rig.clear();
    rig.receive_from_backend(known_packs_question());
    rig.receive_from_backend(registry(1));
    rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
    assert_eq!(rig.sent_to_client(config_cb::FINISH_CONFIGURATION), 0);
    assert_eq!(rig.sent_to_client(config_cb::DISCONNECT), 0);
}

#[test]
fn keep_alives_of_a_configuration_stay_out_of_what_a_replay_follows() {
    let mut rig = Rig::in_configuration();
    rig.receive_from_backend(frame::build(config_cb::KEEP_ALIVE, |body| {
        9i64.encode(body)
    }));
    rig.receive_from_backend(frame::build(config_cb::PING, |body| 4i32.encode(body)));
    rig.lose_backend();
    rig.log_back_in();
    rig.clear();
    rig.receive_from_backend(known_packs_question());
    rig.receive_from_backend(registry(1));
    rig.receive_from_backend(registry(2));
    assert_eq!(rig.client, vec![registry(2)]);
}

#[test]
fn a_crash_after_the_server_asked_for_a_configuration_still_hands_its_frames_over() {
    let mut rig = Rig::in_play();
    rig.receive_from_backend(empty(play_cb::START_CONFIGURATION));
    rig.lose_backend();
    rig.log_back_in();
    rig.clear();
    rig.receive_from_backend(known_packs_question());
    assert!(rig.client.is_empty());
    rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
    assert_eq!(rig.client, vec![known_packs_question()]);
}

#[test]
fn a_configuration_the_server_asked_for_is_what_a_replay_follows() {
    let mut rig = Rig::in_play();
    rig.receive_from_backend(empty(play_cb::START_CONFIGURATION));
    rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
    rig.receive_from_backend(known_packs_question());
    rig.receive_from_client(known_packs_reply());
    rig.receive_from_backend(registry(2));
    rig.lose_backend();
    rig.log_back_in();
    rig.clear();
    rig.receive_from_backend(known_packs_question());
    rig.receive_from_backend(registry(2));
    assert!(rig.client.is_empty());
    assert_eq!(rig.backend, vec![known_packs_reply()]);
}

#[test]
fn a_login_cookie_is_answered_with_nothing() {
    let mut rig = Rig::in_play();
    rig.lose_backend();
    rig.tick(settings().retry_every);
    rig.connected();
    rig.clear();
    rig.receive_from_backend(frame::build(login_cb::COOKIE_REQUEST, |body| {
        "minecraft:token".to_string().encode(body);
    }));
    assert_eq!(
        rig.backend,
        vec![frame::build(login_sb::COOKIE_RESPONSE, |body| {
            "minecraft:token".to_string().encode(body);
            false.encode(body);
        })]
    );
}

#[test]
fn a_server_initiated_configuration_passes_through() {
    let mut rig = Rig::in_play();
    rig.receive_from_backend(empty(play_cb::START_CONFIGURATION));
    assert_eq!(rig.sent_to_client(play_cb::START_CONFIGURATION), 1);
    rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
    assert_eq!(rig.sent_to_backend(play_sb::CONFIGURATION_ACKNOWLEDGED), 1);
    rig.receive_from_backend(registry(3));
    assert_eq!(rig.client.last(), Some(&registry(3)));
}
