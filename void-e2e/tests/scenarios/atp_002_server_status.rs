//! VMC-ATP-002: a status client sees the server discovery information.

use std::net::SocketAddr;
use std::time::Duration;

use voidmc_e2e::TestServer;
use voidmc_e2e::protocol::connect::Connection;
use voidmc_e2e::protocol::packets::handshake::s_intention::ServerboundIntention;
use voidmc_e2e::protocol::packets::status::ClientboundStatusPacket;
use voidmc_e2e::protocol::packets::status::c_status_response::ClientboundStatusResponse;
use voidmc_e2e::protocol::packets::status::s_ping_request::ServerboundPingRequest;
use voidmc_e2e::protocol::packets::status::s_status_request::ServerboundStatusRequest;
use voidmc_e2e::protocol::packets::{ClientIntention, PROTOCOL_VERSION};

/// A refresh slower than this counts as the server timing out. The vanilla
/// multiplayer screen waits longer, so a pass here is a pass there.
const STATUS_DEADLINE: Duration = Duration::from_secs(5);

/// Refreshes the server like the multiplayer screen: handshake with the
/// status intention, ask for the status, then ping on the same connection and
/// wait for the pong carrying the same payload.
async fn refresh(address: SocketAddr) -> ClientboundStatusResponse {
    let exchange = async {
        let mut connection = Connection::new(&address)
            .await
            .unwrap_or_else(|error| panic!("could not connect to {address}: {error}"));
        connection
            .write(ServerboundIntention {
                protocol_version: PROTOCOL_VERSION,
                hostname: address.ip().to_string(),
                port: address.port(),
                intention: ClientIntention::Status,
            })
            .await
            .expect("handshake");
        let mut connection = connection.status();

        connection
            .write(ServerboundStatusRequest {})
            .await
            .expect("status request");
        let status = match connection.read().await {
            Ok(ClientboundStatusPacket::StatusResponse(status)) => status,
            Ok(other) => panic!("expected a status response, got {other:?}"),
            Err(error) => panic!("failed to read the status response: {error}"),
        };

        let time = 0x0123_4567_89ab_cdef;
        connection
            .write(ServerboundPingRequest { time })
            .await
            .expect("ping request");
        match connection.read().await {
            Ok(ClientboundStatusPacket::PongResponse(pong)) => {
                assert_eq!(pong.time, time, "the pong echoes the ping payload")
            }
            Ok(other) => panic!("expected a pong response, got {other:?}"),
            Err(error) => panic!("failed to read the pong response: {error}"),
        }
        status
    };
    tokio::time::timeout(STATUS_DEADLINE, exchange)
        .await
        .unwrap_or_else(|_| panic!("the server-list refresh timed out after {STATUS_DEADLINE:?}"))
}

/// Refreshes the server once per tick until it reports `online` players.
async fn wait_for_online(server: &TestServer, online: i32) {
    let mut reported = refresh(server.address()).await.players.online;
    for _ in 0..100 {
        if reported == online {
            return;
        }
        server.wait_ticks(1).await;
        reported = refresh(server.address()).await.players.online;
    }
    assert_eq!(reported, online, "players online in the server list");
}

#[tokio::test]
async fn server_list_shows_version_motd_and_capacity() {
    // The defaults are what the example server advertises; its own unit test
    // pins its configuration to them.
    let server = TestServer::start().await;

    let status = refresh(server.address()).await;
    assert_eq!(status.version.name, "26.1.2");
    assert_eq!(status.version.protocol, 775);
    assert_eq!(status.description.to_string(), "Welcome to Void Server!");
    assert_eq!(status.players.max, 100);
    assert_eq!(status.players.online, 0);

    server.stop().await;
}

#[tokio::test]
async fn server_list_player_count_follows_joins_and_quits() {
    let server = TestServer::start().await;

    let alice = server.join("Alice").await;
    wait_for_online(&server, 1).await;
    let bob = server.join("Bob").await;
    wait_for_online(&server, 2).await;

    alice.disconnect().await;
    wait_for_online(&server, 1).await;
    bob.disconnect().await;
    wait_for_online(&server, 0).await;

    server.stop().await;
}
