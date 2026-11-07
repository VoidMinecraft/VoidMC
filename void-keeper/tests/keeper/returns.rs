use super::*;

#[tokio::test]
async fn a_client_survives_a_crash_through_a_configuration() {
    let mut plan = Plan::player(1);
    plan.after_login = vec![add_entity(50)];
    let (mut rig, mut server) = rig_with(plan).await;
    let (mut client, _) = FakeClient::join(rig.proxy, "Bot").await;
    client.expect(play_cb::ADD_ENTITY).await;
    server.expect(play_sb::PLAYER_LOADED).await;

    let address = server.crash().await;
    client.expect(play_cb::KEEP_ALIVE).await;
    client.expect(play_cb::SET_ACTION_BAR_TEXT).await;
    rig.expect_event("backend_lost").await;

    let mut plan = Plan::player(9);
    plan.registry = 2;
    let mut server = FakeServer::start(address, plan).await;
    assert_eq!(client.come_back().await, login_packet(9));
    let resumed = rig.expect_event("resumed").await;
    assert!(resumed["outage_ms"].as_u64().is_some());
    assert_eq!(
        client.next().await,
        Frame::encode(play_cb::SET_ACTION_BAR_TEXT, |body| {
            voidmc_keeper::frame::text("").encode(body);
        })
    );

    server.expect(play_sb::PLAYER_LOADED).await;
    server.send(add_entity(60));
    assert_eq!(client.expect(play_cb::ADD_ENTITY).await, add_entity(60));
    client.peer.send(&empty(play_sb::CLIENT_TICK_END)).await;
    server.expect(play_sb::CLIENT_TICK_END).await;
}

#[tokio::test]
async fn a_server_that_stops_cleanly_keeps_the_client_as_a_crash_does() {
    let (mut rig, mut server) = rig_with(Plan::player(1)).await;
    let (mut client, _) = FakeClient::join(rig.proxy, "Bot").await;
    server.expect(play_sb::PLAYER_LOADED).await;

    let address = server.stop().await;
    assert_eq!(client.expect(play_cb::SYSTEM_CHAT).await, chat(0));
    rig.expect_event("backend_lost").await;
    let kept_alive = client.collect_until(play_cb::KEEP_ALIVE).await;
    assert!(
        kept_alive
            .iter()
            .all(|frame| id_of(frame) != play_cb::DISCONNECT)
    );

    let mut server = FakeServer::start(address, Plan::player(1)).await;
    assert_eq!(client.come_back().await, login_packet(1));
    rig.expect_event("resumed").await;
    server.expect(play_sb::PLAYER_LOADED).await;
}

#[tokio::test]
async fn three_restarts_in_a_row_each_go_through_a_configuration() {
    let (mut rig, server) = rig_with(Plan::player(1)).await;
    let (mut client, _) = FakeClient::join(rig.proxy, "Bot").await;
    let mut server = server;
    for _ in 0..3 {
        server.expect(play_sb::PLAYER_LOADED).await;
        let address = server.crash().await;
        rig.expect_event("backend_lost").await;
        server = FakeServer::start(address, Plan::player(1)).await;
        assert_eq!(client.come_back().await, login_packet(1));
        rig.expect_event("resumed").await;
    }
}

#[tokio::test]
async fn two_clients_come_back_on_their_own() {
    let (mut rig, server) = rig_with(Plan::player(1)).await;
    let (mut first, _) = FakeClient::join(rig.proxy, "Bot").await;
    let (mut second, _) = FakeClient::join(rig.proxy, "Other").await;
    assert_eq!(server.logins(), 2);
    let address = server.crash().await;

    first.expect(play_cb::KEEP_ALIVE).await;
    second.expect(play_cb::KEEP_ALIVE).await;
    let server = FakeServer::start(address, Plan::player(1)).await;
    first.come_back().await;
    second.come_back().await;
    for _ in 0..2 {
        rig.expect_event("resumed").await;
    }
    assert_eq!(server.logins(), 2);
    server.send(add_entity(5));
    assert_eq!(first.expect(play_cb::ADD_ENTITY).await, add_entity(5));
    assert_eq!(second.expect(play_cb::ADD_ENTITY).await, add_entity(5));
}

#[tokio::test]
async fn a_client_that_leaves_while_the_server_is_away_is_forgotten() {
    let (mut rig, server) = rig_with(Plan::player(1)).await;
    let (leaving, _) = FakeClient::join(rig.proxy, "Bot").await;
    let (mut staying, _) = FakeClient::join(rig.proxy, "Other").await;
    let address = server.crash().await;
    rig.expect_event("backend_lost").await;

    drop(leaving);
    rig.expect_event("client_left").await;

    let server = FakeServer::start(address, Plan::player(1)).await;
    staying.come_back().await;
    assert_eq!(rig.expect_event("resumed").await["session"], 2);
    sleep(Duration::from_millis(100)).await;
    assert_eq!(server.logins(), 1);
}

#[tokio::test]
async fn a_compressing_server_stays_invisible_to_the_client() {
    let mut plan = Plan::player(1);
    plan.compression = Some(64);
    plan.after_login = vec![add_entity(50)];
    let (mut rig, mut server) = rig_with(plan.clone()).await;
    let (mut client, login) = FakeClient::join(rig.proxy, "Bot").await;
    assert_eq!(login, login_packet(1));
    client.expect(play_cb::ADD_ENTITY).await;
    server.expect(play_sb::PLAYER_LOADED).await;

    let large = Frame::encode(play_cb::SYSTEM_CHAT, |body| {
        body.extend_from_slice(&[9; 5000])
    });
    server.send(large.clone());
    assert_eq!(client.expect(play_cb::SYSTEM_CHAT).await, large);

    let address = server.crash().await;
    let mut server = FakeServer::start(address, plan).await;
    client.come_back().await;
    rig.expect_event("resumed").await;
    server.expect(play_sb::PLAYER_LOADED).await;
    client.peer.send(&vec![0u8; 5000]).await;
    server.expect(0).await;
}

#[tokio::test]
async fn a_change_of_compression_between_two_runs_is_followed() {
    let (mut rig, server) = rig_with(Plan::player(1)).await;
    let (mut client, _) = FakeClient::join(rig.proxy, "Bot").await;
    let address = server.crash().await;
    let mut plan = Plan::player(1);
    plan.compression = Some(64);
    let server = FakeServer::start(address, plan).await;
    client.come_back().await;
    rig.expect_event("resumed").await;
    let large = Frame::encode(play_cb::SYSTEM_CHAT, |body| {
        body.extend_from_slice(&[9; 5000])
    });
    server.send(large.clone());
    assert_eq!(client.expect(play_cb::SYSTEM_CHAT).await, large);
}

#[tokio::test]
async fn a_client_arriving_before_its_server_waits_for_it() {
    let port_holder = TcpListener::bind("[::1]:0").await.unwrap();
    let address = port_holder.local_addr().unwrap();
    drop(port_holder);
    let rig = Rig::start(address).await;
    let joining = tokio::spawn(async move { FakeClient::join(rig.proxy, "Bot").await });
    sleep(Duration::from_millis(100)).await;
    let _server = FakeServer::start(address, Plan::player(1)).await;
    let (_client, login) = within(joining).await.unwrap();
    assert_eq!(login, login_packet(1));
}
