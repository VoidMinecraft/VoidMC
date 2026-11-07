use super::*;

#[tokio::test]
async fn the_return_of_a_server_is_announced_without_any_client() {
    let (mut rig, server) = rig_with(Plan::player(1)).await;
    rig.expect_event("backend_up").await;
    let address = server.crash().await;
    rig.expect_event("backend_down").await;
    let _server = FakeServer::start(address, Plan::player(1)).await;
    rig.expect_event("backend_up").await;
}

#[tokio::test]
async fn a_status_request_is_answered_while_the_server_is_away() {
    let (rig, server) = rig_with(Plan::player(1)).await;
    server.crash().await;
    let mut client = FakeClient::connect(rig.proxy, 1, PROTOCOL_VERSION).await;
    client.peer.send(&empty(status_sb::STATUS_REQUEST)).await;
    let status = client.expect(status_cb::STATUS_RESPONSE).await;
    assert!(String::from_utf8_lossy(&status).contains("away"));
    client
        .peer
        .send(&frame::build(status_sb::PING_REQUEST, |body| {
            3i64.encode(body)
        }))
        .await;
    client.expect(status_cb::PONG_RESPONSE).await;
}

#[tokio::test]
async fn a_status_request_reaches_a_live_server() {
    let (rig, _server) = rig_with(Plan::player(1)).await;
    let mut client = FakeClient::connect(rig.proxy, 1, PROTOCOL_VERSION).await;
    client.peer.send(&empty(status_sb::STATUS_REQUEST)).await;
    let status = client.expect(status_cb::STATUS_RESPONSE).await;
    assert!(!String::from_utf8_lossy(&status).contains("away"));
}

#[tokio::test]
async fn another_protocol_is_refused_with_a_reason() {
    let (rig, _server) = rig_with(Plan::player(1)).await;
    let mut client = FakeClient::connect(rig.proxy, 2, PROTOCOL_VERSION - 1).await;
    let refusal = client.expect(login_cb::LOGIN_DISCONNECT).await;
    assert!(String::from_utf8_lossy(&refusal).contains("protocol"));
}

#[tokio::test]
async fn the_server_is_not_probed_while_a_client_plays() {
    let (rig, server) = rig_with(Plan::player(1)).await;
    within(async {
        while server.probes() == 0 {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    let (client, _) = FakeClient::join(rig.proxy, "Bot").await;
    sleep(Duration::from_millis(200)).await;
    let probed = server.probes();
    sleep(Duration::from_millis(1500)).await;
    assert_eq!(server.probes(), probed);

    drop(client);
    within(async {
        while server.probes() == probed {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
}
