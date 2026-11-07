use tokio::net::TcpSocket;

use super::*;

#[tokio::test]
async fn a_server_that_never_returns_ends_with_a_clear_message() {
    let server = FakeServer::start("[::1]:0".parse().unwrap(), Plan::player(1)).await;
    let patience = Duration::from_millis(600);
    let mut settings = settings();
    settings.return_timeout = patience;
    let mut rig = Rig::start_with(server.address, settings).await;
    let (mut client, _) = FakeClient::join(rig.proxy, "Bot").await;
    let started = Instant::now();
    server.crash().await;

    let goodbye = client.expect(play_cb::DISCONNECT).await;
    assert!(started.elapsed() >= patience);
    let text = String::from_utf8_lossy(&goodbye);
    assert!(text.contains("did not come back"), "{text}");
    assert!(within(client.peer.next()).await.is_none());
    rig.expect_event("gave_up").await;
}

#[tokio::test]
async fn a_packet_the_proxy_cannot_read_still_reaches_the_client() {
    let (rig, server) = rig_with(Plan::player(1)).await;
    let (mut client, _) = FakeClient::join(rig.proxy, "Bot").await;
    let broken = frame::encode(play_cb::RESOURCE_PACK_PUSH, |body| body.push(1));
    server.send(broken.clone());
    assert_eq!(client.expect(play_cb::RESOURCE_PACK_PUSH).await, broken);
}

#[tokio::test]
async fn garbage_from_the_server_is_a_lost_server_not_a_crash() {
    let listener = TcpListener::bind("[::1]:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut rig = Rig::start(address).await;
    let accepting = tokio::spawn(async move {
        let mut peer = accept_login(&listener).await;
        peer.next().await;
        peer.write.write_all(&[0xFF; 16]).await.unwrap();
        peer
    });
    let mut client = FakeClient::connect(rig.proxy, 2, PROTOCOL_VERSION).await;
    client
        .peer
        .send(&frame::encode(login_sb::HELLO, |body| {
            "Bot".to_string().encode(body);
            Uuid::nil().encode(body);
        }))
        .await;
    let _stuck = within(accepting).await.unwrap();
    let server = FakeServer::start(address, Plan::player(1)).await;
    client.expect(login_cb::LOGIN_FINISHED).await;
    assert_eq!(server.logins(), 1);
    let (_healthy, _) = FakeClient::join(rig.proxy, "Other").await;
    rig.expect_event("client_joined").await;
}

#[tokio::test]
async fn a_frame_cut_by_the_crash_never_reaches_the_client() {
    let listener = TcpListener::bind("[::1]:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let rig = Rig::start(address).await;
    let server = tokio::spawn(async move {
        let mut peer = accept_login(&listener).await;
        log_in_by_hand(&mut peer).await;
        peer.write.write_all(&[200, 1, 5, 5, 5]).await.unwrap();
    });
    let (mut client, _) = FakeClient::join(rig.proxy, "Bot").await;
    within(server).await.unwrap();
    assert_eq!(id_of(&client.next().await), play_cb::KEEP_ALIVE);
}

#[tokio::test]
async fn a_server_lost_after_a_registry_does_not_hand_it_to_the_client_twice() {
    let listener = TcpListener::bind("[::1]:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut rig = Rig::start(address).await;
    let first = tokio::spawn(async move {
        let mut peer = accept_login(&listener).await;
        peer.next().await;
        peer.send(&login_finished()).await;
        peer.next().await;
        peer.next().await;
        peer.send(&frame::encode(config_cb::SELECT_KNOWN_PACKS, |body| {
            body.push(0)
        }))
        .await;
        peer.next().await;
        peer.send(&registry(1)).await;
        sleep(Duration::from_millis(100)).await;
    });
    let proxy = rig.proxy;
    let joining = tokio::spawn(async move { FakeClient::join(proxy, "Bot").await });
    within(first).await.unwrap();

    let _server = FakeServer::start(address, Plan::player(1)).await;
    let (_client, login) = within(joining).await.unwrap();
    assert_eq!(login, login_packet(1));
    rig.expect_event("resumed").await;
}

#[tokio::test]
async fn a_silent_server_has_its_connection_closed() {
    let listener = TcpListener::bind("[::1]:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let silence = Duration::from_millis(300);
    let mut settings = settings();
    settings.backend_silence = silence;
    settings.retry_every = Duration::from_secs(5);
    settings.return_timeout = Duration::from_secs(20);
    let rig = Rig::start_with(address, settings).await;
    let server = tokio::spawn(async move {
        let mut peer = accept_login(&listener).await;
        log_in_by_hand(&mut peer).await;
        let silent_since = Instant::now();
        while peer.next().await.is_some() {}
        (silent_since.elapsed(), listener)
    });
    let (_client, _) = FakeClient::join(rig.proxy, "Bot").await;
    let (closed_after, _listener) = within(server).await.unwrap();
    assert!(closed_after >= silence / 2, "{closed_after:?}");
    assert!(
        closed_after < silence + Duration::from_secs(2),
        "{closed_after:?}"
    );
}

#[tokio::test]
async fn a_server_that_stops_reading_counts_as_lost() {
    let socket = TcpSocket::new_v6().unwrap();
    socket.set_recv_buffer_size(4096).unwrap();
    socket.bind("[::1]:0".parse().unwrap()).unwrap();
    let listener = socket.listen(16).unwrap();
    let address = listener.local_addr().unwrap();
    let mut rig = Rig::start(address).await;
    let server = tokio::spawn(async move {
        let mut peer = accept_login(&listener).await;
        log_in_by_hand(&mut peer).await;
        (peer, listener)
    });
    let (mut client, _) = FakeClient::join(rig.proxy, "Bot").await;
    let (_deaf, _listener) = within(server).await.unwrap();
    let flood = frame::encode(play_sb::CLIENT_TICK_END, |body| {
        body.extend_from_slice(&[0; 4000])
    });
    let flooding = tokio::spawn(async move {
        for _ in 0..8000 {
            client.peer.send(&flood).await;
        }
    });
    rig.expect_event_within("backend_lost", WRITE_TIMEOUT + PATIENCE)
        .await;
    flooding.abort();
}
