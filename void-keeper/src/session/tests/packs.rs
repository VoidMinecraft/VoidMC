use super::*;

#[test]
fn a_pack_pushed_during_a_replay_reaches_the_client_and_the_replay_goes_on() {
    let pack = Uuid::from_u128(5);
    let mut rig = Rig::in_configuration();
    rig.lose_backend();
    rig.log_back_in();
    rig.clear();

    rig.receive_from_backend(config_push(pack));
    assert_eq!(rig.sent_to_client(config_cb::RESOURCE_PACK_PUSH), 1);
    rig.receive_from_client(config_pack_answer(pack, 0));
    assert_eq!(rig.sent_to_backend(config_sb::RESOURCE_PACK), 1);

    rig.receive_from_backend(known_packs_question());
    rig.receive_from_backend(registry(1));
    assert_eq!(rig.sent_to_client(config_cb::REGISTRY_DATA), 0);
    assert_eq!(rig.sent_to_client(config_cb::DISCONNECT), 0);
}

#[test]
fn a_pack_the_client_took_stays_out_of_what_a_replay_follows() {
    let pack = Uuid::from_u128(5);
    let mut rig = Rig::fresh();
    rig.tick(Duration::ZERO);
    rig.connected();
    rig.receive_from_client(hello());
    rig.receive_from_backend(login_finished());
    rig.receive_from_client(empty(login_sb::LOGIN_ACKNOWLEDGED));
    rig.receive_from_backend(config_push(pack));
    rig.receive_from_client(config_pack_answer(pack, 0));
    rig.receive_from_backend(known_packs_question());
    rig.receive_from_client(known_packs_reply());
    rig.receive_from_backend(registry(1));
    rig.lose_backend();
    rig.log_back_in();
    rig.clear();

    rig.receive_from_backend(known_packs_question());
    rig.receive_from_backend(config_push(pack));
    rig.receive_from_backend(registry(1));
    assert_eq!(
        rig.backend,
        vec![known_packs_reply(), config_pack_answer(pack, 0)]
    );
    assert!(rig.client.is_empty());
}

#[test]
fn a_pack_for_a_client_that_left_its_configuration_ends_the_session() {
    let mut rig = Rig::in_configuration();
    rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
    rig.lose_backend();
    rig.log_back_in();
    rig.clear();

    rig.receive_from_backend(config_push(Uuid::from_u128(5)));
    assert_eq!(rig.sent_to_client(config_cb::RESOURCE_PACK_PUSH), 0);
    assert_eq!(rig.sent_to_client(play_cb::DISCONNECT), 1);
    assert!(rig.rest.contains(&Action::DropBackend));
    assert!(rig.rest.contains(&Action::Close));
}

#[test]
fn a_client_waiting_for_its_packs_does_not_make_a_server_silent() {
    let mut rig = Rig::in_configuration();
    rig.receive_from_backend(config_push(Uuid::from_u128(5)));
    assert_eq!(rig.sent_to_client(config_cb::RESOURCE_PACK_PUSH), 1);
    rig.clear();
    rig.tick(settings().backend_silence);
    assert!(rig.rest.is_empty());
}

#[test]
fn a_client_downloading_a_pack_during_a_return_does_not_make_a_server_silent() {
    let pack = Uuid::from_u128(9);
    let mut rig = Rig::in_play();
    rig.lose_backend();
    rig.log_back_in();
    rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
    rig.receive_from_backend(config_push(pack));
    rig.receive_from_client(config_pack_answer(pack, 3));
    rig.clear();
    rig.tick(settings().backend_silence);
    assert!(rig.rest.is_empty());
    rig.receive_from_client(config_pack_answer(pack, 0));
    rig.tick(Duration::ZERO);
    assert!(rig.rest.contains(&Action::DropBackend));
}

#[test]
fn a_pack_the_client_holds_is_answered_in_its_name() {
    let pack = Uuid::from_u128(5);
    let mut rig = Rig::in_play();
    rig.receive_from_backend(push(pack, "abc"));
    assert_eq!(rig.sent_to_client(play_cb::RESOURCE_PACK_PUSH), 1);
    for status in [3, 4, 0] {
        rig.receive_from_client(pack_answer(pack, status));
    }
    rig.clear();
    rig.lose_backend();
    rig.come_back();
    rig.clear();

    rig.receive_from_backend(push(pack, "abc"));
    assert_eq!(rig.sent_to_client(play_cb::RESOURCE_PACK_PUSH), 0);
    assert_eq!(
        rig.backend,
        vec![
            pack_answer(pack, 3),
            pack_answer(pack, 4),
            pack_answer(pack, 0)
        ]
    );
}

fn a_pack_the_client_failed_reaches_it_again_after_a_return(failure: i32) {
    let pack = Uuid::from_u128(5);
    let mut rig = Rig::in_play();
    rig.receive_from_backend(push(pack, "abc"));
    rig.receive_from_client(pack_answer(pack, 3));
    rig.receive_from_client(pack_answer(pack, failure));
    rig.lose_backend();
    rig.come_back();
    rig.clear();

    rig.receive_from_backend(push(pack, "abc"));
    assert_eq!(rig.client, vec![push(pack, "abc")]);
    assert!(rig.backend.is_empty());
}

#[test]
fn a_pack_the_client_failed_to_download_reaches_it_again() {
    a_pack_the_client_failed_reaches_it_again_after_a_return(2);
}

#[test]
fn a_pack_with_an_invalid_url_reaches_the_client_again() {
    a_pack_the_client_failed_reaches_it_again_after_a_return(5);
}

#[test]
fn a_pack_the_client_failed_to_reload_reaches_it_again() {
    a_pack_the_client_failed_reaches_it_again_after_a_return(6);
}

#[test]
fn a_pack_the_client_discarded_reaches_it_again() {
    a_pack_the_client_failed_reaches_it_again_after_a_return(7);
}

#[test]
fn a_pack_the_client_holds_is_answered_in_its_name_before_it_acknowledges() {
    let pack = Uuid::from_u128(5);
    let mut rig = Rig::in_play();
    rig.receive_from_backend(push(pack, "abc"));
    rig.receive_from_client(pack_answer(pack, 0));
    rig.lose_backend();
    rig.log_back_in();
    rig.clear();

    rig.receive_from_backend(config_push(pack));
    assert_eq!(rig.backend, vec![config_pack_answer(pack, 0)]);
    rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
    assert_eq!(rig.sent_to_client(config_cb::RESOURCE_PACK_PUSH), 0);
}

#[test]
fn a_new_pack_reaches_the_client_and_the_old_one_goes_when_things_settle() {
    let (old, new) = (Uuid::from_u128(5), Uuid::from_u128(6));
    let mut rig = Rig::in_play();
    rig.receive_from_backend(push(old, "abc"));
    rig.receive_from_client(pack_answer(old, 0));
    rig.lose_backend();
    rig.come_back();
    rig.clear();
    rig.receive_from_backend(push(new, "def"));
    assert_eq!(rig.sent_to_client(play_cb::RESOURCE_PACK_PUSH), 1);
    rig.tick(Duration::from_secs(2));
    assert_eq!(rig.sent_to_client(play_cb::RESOURCE_PACK_POP), 0);
    rig.tick(Duration::from_secs(2));
    assert_eq!(
        rig.client.last(),
        Some(&frame::encode(play_cb::RESOURCE_PACK_POP, |body| Some(old).encode(body)))
    );
}

#[test]
fn a_new_pack_during_a_configuration_waits_for_the_client() {
    let mut rig = Rig::in_play();
    rig.lose_backend();
    rig.log_back_in();
    rig.clear();
    rig.receive_from_backend(config_push(Uuid::from_u128(9)));
    assert!(rig.backend.is_empty());
    rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
    assert_eq!(rig.client, vec![config_push(Uuid::from_u128(9))]);
}

#[test]
fn a_pack_taken_back_during_a_reconfiguration_is_no_longer_held() {
    let pack = Uuid::from_u128(5);
    let mut rig = Rig::in_play();
    rig.receive_from_backend(push(pack, "abc"));
    rig.receive_from_client(pack_answer(pack, 0));
    rig.lose_backend();
    rig.log_back_in();
    rig.receive_from_backend(known_packs_question());
    rig.receive_from_backend(frame::encode(config_cb::RESOURCE_PACK_POP, |body| {
        Some(pack).encode(body);
    }));
    rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
    rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
    rig.receive_from_client(known_packs_reply());
    rig.receive_from_client(empty(config_sb::FINISH_CONFIGURATION));
    rig.receive_from_backend(login());
    rig.clear();

    rig.receive_from_backend(push(pack, "abc"));
    assert_eq!(rig.sent_to_client(play_cb::RESOURCE_PACK_PUSH), 1);
}
