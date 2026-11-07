use super::*;

#[test]
fn a_first_join_passes_through_untouched() {
    let mut rig = Rig::fresh();
    rig.tick(Duration::ZERO);
    assert_eq!(rig.rest, vec![Action::Connect]);
    rig.connected();
    rig.receive_from_client(hello());
    assert_eq!(rig.backend, vec![empty(0), hello()]);
    rig.receive_from_backend(known_packs_question());
    assert_eq!(rig.client, vec![known_packs_question()]);
}

#[test]
fn a_client_is_kept_alive_while_the_server_is_away() {
    let mut rig = Rig::in_play();
    rig.lose_backend();
    assert_eq!(rig.events(), vec![&Event::BackendLost]);
    rig.tick(Duration::ZERO);
    assert_eq!(rig.sent_to_client(play_cb::KEEP_ALIVE), 1);
    assert_eq!(rig.sent_to_client(play_cb::SET_ACTION_BAR_TEXT), 1);
    rig.tick(Duration::from_millis(500));
    assert_eq!(rig.sent_to_client(play_cb::KEEP_ALIVE), 1);
    rig.tick(Duration::from_millis(600));
    assert_eq!(rig.sent_to_client(play_cb::KEEP_ALIVE), 2);
    assert!(rig.rest.contains(&Action::Connect));
}

#[test]
fn nothing_from_the_client_reaches_a_server_that_is_not_there() {
    let mut rig = Rig::in_play();
    rig.lose_backend();
    rig.backend.clear();
    rig.receive_from_client(empty(play_sb::CHAT_COMMAND));
    assert!(rig.backend.is_empty());
}

#[test]
fn the_answer_to_our_keep_alive_never_reaches_the_new_server() {
    let mut rig = Rig::in_play();
    let answer = frame::encode(play_sb::KEEP_ALIVE, |body| {
        (OWN_KEEP_ALIVE | 3).encode(body);
    });
    rig.receive_from_client(answer);
    assert!(rig.backend.is_empty());
    let theirs = frame::encode(play_sb::KEEP_ALIVE, |body| {
        1_700_000_000_000i64.encode(body)
    });
    rig.receive_from_client(theirs.clone());
    assert_eq!(rig.backend, vec![theirs]);
}

#[test]
fn connecting_is_retried_at_its_own_pace() {
    let mut rig = Rig::in_play();
    rig.lose_backend();
    rig.tick(Duration::from_millis(100));
    rig.tick(Duration::from_millis(100));
    assert_eq!(
        rig.rest.iter().filter(|a| **a == Action::Connect).count(),
        1
    );
    rig.session.on_connect_failed(rig.now);
    rig.tick(Duration::from_millis(50));
    rig.tick(Duration::from_millis(50));
    assert_eq!(
        rig.rest.iter().filter(|a| **a == Action::Connect).count(),
        2
    );
}

#[test]
fn a_frozen_server_that_accepts_the_connection_leaves_the_client_in_its_world() {
    let mut rig = Rig::in_play();
    rig.lose_backend();
    rig.tick(settings().retry_every);
    rig.connected();
    rig.clear();
    for _ in 0..4 {
        rig.tick(Duration::from_secs(1));
    }
    assert_eq!(rig.sent_to_client(play_cb::START_CONFIGURATION), 0);
    assert_eq!(rig.sent_to_client(play_cb::KEEP_ALIVE), 4);
    assert_eq!(rig.sent_to_client(play_cb::SET_ACTION_BAR_TEXT), 4);
    rig.tick(Duration::from_secs(1));
    assert!(rig.rest.contains(&Action::DropBackend));
    rig.clear();
    rig.tick(settings().retry_every);
    assert!(rig.rest.contains(&Action::Connect));
    assert_eq!(rig.sent_to_client(play_cb::START_CONFIGURATION), 0);
}

#[test]
fn a_server_still_starting_is_tried_again() {
    let mut rig = Rig::in_play();
    rig.lose_backend();
    rig.tick(settings().retry_every);
    rig.connected();
    rig.clear();
    rig.receive_from_backend(frame::encode(login_cb::LOGIN_DISCONNECT, |body| {
        "starting".to_string().encode(body);
    }));
    assert!(rig.rest.contains(&Action::DropBackend));
    rig.tick(settings().retry_every);
    assert!(rig.rest.contains(&Action::Connect));
}

#[test]
fn a_crash_in_the_middle_of_the_return_starts_the_wait_again() {
    let mut rig = Rig::in_play();
    rig.lose_backend();
    rig.log_back_in();
    rig.receive_from_backend(known_packs_question());
    rig.clear();
    rig.lose_backend();
    assert!(rig.events().is_empty());
    rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
    assert!(rig.client.is_empty());
    rig.log_back_in();
    assert_eq!(rig.sent_to_client(play_cb::START_CONFIGURATION), 0);
    rig.client.clear();
    rig.receive_from_backend(known_packs_question());
    assert_eq!(rig.client, vec![known_packs_question()]);
    rig.receive_from_client(known_packs_reply());
    rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
    rig.receive_from_client(empty(config_sb::FINISH_CONFIGURATION));
    assert_eq!(rig.resumed(), 1);
}

#[test]
fn a_server_that_never_returns_ends_with_an_explanation_in_play() {
    let mut rig = Rig::in_play();
    rig.lose_backend();
    rig.tick(Duration::from_secs(9));
    assert_eq!(rig.sent_to_client(play_cb::DISCONNECT), 0);
    rig.tick(Duration::from_secs(2));
    assert_eq!(rig.sent_to_client(play_cb::DISCONNECT), 1);
    assert!(matches!(rig.rest.last(), Some(Action::Close)));
    assert!(matches!(rig.events()[1], Event::GaveUp { .. }));
    rig.clear();
    rig.tick(Duration::from_secs(5));
    assert!(rig.rest.is_empty() && rig.client.is_empty());
}

#[test]
fn a_client_told_it_logged_in_is_not_told_twice() {
    let mut rig = Rig::fresh();
    rig.tick(Duration::ZERO);
    rig.connected();
    rig.receive_from_client(hello());
    rig.receive_from_backend(login_finished());
    rig.clear();
    rig.lose_backend();
    rig.tick(settings().retry_every);
    assert!(!rig.rest.contains(&Action::Connect));

    rig.receive_from_client(empty(login_sb::LOGIN_ACKNOWLEDGED));
    assert!(rig.backend.is_empty());
    rig.clear();
    rig.log_back_in();
    assert_eq!(rig.sent_to_client(login_cb::LOGIN_FINISHED), 0);
    assert_eq!(rig.sent_to_backend(login_sb::LOGIN_ACKNOWLEDGED), 1);
    rig.client.clear();
    rig.receive_from_backend(known_packs_question());
    assert_eq!(rig.client, vec![known_packs_question()]);
}

#[test]
fn a_silent_server_in_play_counts_as_lost() {
    let mut rig = Rig::in_play();
    rig.tick(settings().backend_silence - Duration::from_secs(1));
    assert!(rig.rest.is_empty());
    rig.tick(Duration::from_secs(1));
    assert!(rig.rest.contains(&Action::DropBackend));
    assert_eq!(rig.events(), vec![&Event::BackendLost]);
}

#[test]
fn a_server_that_freezes_during_the_return_is_tried_again() {
    let mut rig = Rig::in_play();
    rig.lose_backend();
    rig.log_back_in();
    rig.clear();
    rig.tick(settings().backend_silence);
    assert!(rig.rest.contains(&Action::DropBackend));
    rig.tick(settings().retry_every);
    assert!(rig.rest.contains(&Action::Connect));
    assert_eq!(rig.sent_to_client(play_cb::DISCONNECT), 0);
}

#[test]
fn a_disconnect_inside_a_bundle_closes_the_bundle_first() {
    let mut rig = Rig::in_play();
    rig.receive_from_backend(empty(play_cb::BUNDLE_DELIMITER));
    rig.clear();
    rig.receive_from_backend(frame::encode(play_cb::SYSTEM_CHAT, |body| {
        body.resize(MAX_PLAIN_FRAME_BYTES + 1, 0);
    }));
    let ids: Vec<i32> = rig
        .client
        .iter()
        .map(|f| frame::id_and_body(f).unwrap().0)
        .collect();
    assert_eq!(ids, vec![play_cb::BUNDLE_DELIMITER, play_cb::DISCONNECT]);
}

#[test]
fn a_kick_from_a_live_server_ends_the_session_without_a_loss() {
    let mut rig = Rig::in_play();
    let kick = frame::encode(play_cb::DISCONNECT, |body| {
        text("Kicked").encode(body);
    });
    rig.receive_from_backend(kick.clone());
    assert_eq!(rig.client, vec![kick]);
    assert!(rig.rest.contains(&Action::DropBackend));
    assert!(rig.rest.contains(&Action::Close));
    rig.lose_backend();
    assert!(rig.events().is_empty());
}

#[test]
fn a_packet_a_client_could_not_read_ends_the_session_with_its_size() {
    let mut rig = Rig::in_play();
    let large = frame::encode(play_cb::SYSTEM_CHAT, |body| {
        body.resize(MAX_PLAIN_FRAME_BYTES + 1, 0);
    });
    rig.receive_from_backend(large);
    assert_eq!(rig.sent_to_client(play_cb::SYSTEM_CHAT), 0);
    assert_eq!(rig.sent_to_client(play_cb::DISCONNECT), 1);
    assert!(rig.rest.contains(&Action::DropBackend));
    assert!(rig.rest.contains(&Action::Close));
    assert!(matches!(rig.events()[..], [Event::GaveUp { .. }]));
}

#[test]
fn the_largest_packet_a_client_reads_still_reaches_it() {
    let mut rig = Rig::in_play();
    let largest = frame::encode(play_cb::SYSTEM_CHAT, |body| {
        body.resize(MAX_PLAIN_FRAME_BYTES, 0);
    });
    rig.receive_from_backend(largest);
    assert_eq!(rig.sent_to_client(play_cb::SYSTEM_CHAT), 1);
}

#[test]
fn the_away_message_goes_once_the_client_is_back() {
    let mut rig = Rig::in_play();
    rig.lose_backend();
    rig.tick(Duration::ZERO);
    assert_eq!(rig.sent_to_client(play_cb::SET_ACTION_BAR_TEXT), 1);
    rig.come_back();
    rig.clear();
    rig.receive_from_backend(empty(play_cb::START_CONFIGURATION));
    rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
    rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
    rig.receive_from_client(empty(config_sb::FINISH_CONFIGURATION));
    rig.receive_from_backend(login());
    assert_eq!(rig.sent_to_client(play_cb::SET_ACTION_BAR_TEXT), 0);
}

#[test]
fn a_server_heard_every_second_is_never_counted_as_lost() {
    let mut rig = Rig::in_play();
    for _ in 0..30 {
        rig.tick(Duration::from_secs(1));
        rig.receive_from_backend(frame::encode(play_cb::KEEP_ALIVE, |body| 1i64.encode(body)));
    }
    assert!(!rig.rest.contains(&Action::DropBackend));
    assert!(rig.events().is_empty());
}

#[test]
fn a_bundle_cut_by_a_crash_is_closed_before_anything_else() {
    let mut rig = Rig::in_play();
    rig.receive_from_backend(empty(play_cb::BUNDLE_DELIMITER));
    rig.clear();
    rig.lose_backend();
    assert_eq!(rig.client, vec![empty(play_cb::BUNDLE_DELIMITER)]);
}

#[test]
fn a_closed_bundle_is_left_alone() {
    let mut rig = Rig::in_play();
    rig.receive_from_backend(empty(play_cb::BUNDLE_DELIMITER));
    rig.receive_from_backend(empty(play_cb::BUNDLE_DELIMITER));
    rig.clear();
    rig.lose_backend();
    assert!(rig.client.is_empty());
}

#[test]
fn compression_stays_on_the_backend_side() {
    let mut rig = Rig::fresh();
    rig.tick(Duration::ZERO);
    rig.connected();
    rig.receive_from_backend(frame::encode(login_cb::LOGIN_COMPRESSION, |body| {
        VarI32(256).encode(body);
    }));
    assert!(rig.rest.contains(&Action::Compress(256)));
    assert!(rig.client.is_empty());
}

#[test]
fn compression_is_followed_on_a_return_too() {
    let mut rig = Rig::in_play();
    rig.lose_backend();
    rig.tick(settings().retry_every);
    rig.connected();
    rig.clear();
    rig.receive_from_backend(frame::encode(login_cb::LOGIN_COMPRESSION, |body| {
        VarI32(64).encode(body);
    }));
    assert!(rig.rest.contains(&Action::Compress(64)));
}

#[test]
fn a_second_crash_long_after_a_return_gets_the_whole_return_timeout() {
    let mut rig = Rig::in_play();
    rig.lose_backend();
    rig.come_back();
    rig.tick(Duration::from_secs(30));
    rig.lose_backend();
    rig.clear();
    rig.tick(Duration::from_secs(9));
    assert_eq!(rig.sent_to_client(play_cb::DISCONNECT), 0);
    rig.tick(Duration::from_secs(2));
    assert_eq!(rig.sent_to_client(play_cb::DISCONNECT), 1);
}

#[test]
fn frames_without_an_id_change_nothing() {
    let mut rig = Rig::in_play();
    rig.receive_from_backend(Bytes::from_static(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]));
    rig.receive_from_client(Bytes::from_static(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]));
    assert!(rig.client.is_empty() && rig.backend.is_empty());
}
