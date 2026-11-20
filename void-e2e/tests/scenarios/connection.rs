use std::future::Future;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use voidmc::components::{KeepAliveState, PlayerReady, PlayerUuid};
use voidmc::network::PrePlayTimeout;
use voidmc::systems::KeepAliveTicker;
use voidmc_e2e::TestServer;
use voidmc_e2e::protocol::common::client_information::ClientInformation;
use voidmc_e2e::protocol::connect::Connection;
use voidmc_e2e::protocol::packets::config::s_client_information::ServerboundClientInformation;
use voidmc_e2e::protocol::packets::game::ClientboundGamePacket;
use voidmc_e2e::protocol::packets::handshake::s_intention::ServerboundIntention;
use voidmc_e2e::protocol::packets::handshake::{
    ClientboundHandshakePacket, ServerboundHandshakePacket,
};
use voidmc_e2e::protocol::packets::login::ClientboundLoginPacket;
use voidmc_e2e::protocol::packets::login::s_hello::ServerboundHello;
use voidmc_e2e::protocol::packets::{ClientIntention, PROTOCOL_VERSION};

use crate::{DECODE_FAILURE, feet, ground, malformed_play_packets, raw, var_int};

async fn join_and_spawn_on_the_ground(threshold: Option<u32>) {
    let server = TestServer::builder()
        .compression_threshold(threshold)
        .start()
        .await;
    let alice = server.join("Alice").await;

    let transcript = alice.transcript();
    assert_eq!(
        transcript.compression_threshold,
        threshold.map(|threshold| threshold as i32)
    );
    assert_eq!(
        transcript.known_packs,
        [format!(
            "minecraft:core@{}",
            voidmc_protocol::MINECRAFT_VERSION
        )]
    );
    assert!(
        transcript
            .registries
            .iter()
            .any(|id| id == "minecraft:dimension_type")
    );
    assert!(transcript.tags_received);

    assert_eq!(
        ground(&alice).y,
        feet(&alice).y - 1,
        "spawns standing on the ground"
    );

    alice.sync().await;
    alice.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn joins_without_compression() {
    join_and_spawn_on_the_ground(None).await;
}

#[tokio::test]
async fn joins_with_compression() {
    join_and_spawn_on_the_ground(Some(64)).await;
}

#[tokio::test]
async fn joins_with_every_packet_compressed() {
    join_and_spawn_on_the_ground(Some(0)).await;
}

#[tokio::test]
async fn server_list_reports_the_players_online() {
    let server = TestServer::builder()
        .config(|config| config.max_players(7).motd("e2e"))
        .start()
        .await;
    let _alice = server.join("Alice").await;
    let _bob = server.join("Bob").await;

    let status = server.status().await;
    assert_eq!(status.version.protocol, voidmc_protocol::PROTOCOL_VERSION);
    assert_eq!(status.players.max, 7);
    assert_eq!(status.description.to_string(), "e2e");
    let mut online = server.status().await.players.online;
    for _ in 0..100 {
        if online == 2 {
            break;
        }
        server.wait_ticks(1).await;
        online = server.status().await.players.online;
    }
    assert_eq!(online, 2);

    server.stop().await;
}

#[tokio::test]
async fn keep_alive_round_trips_and_keeps_going() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let entity = server.player_entity(alice.uuid()).await;
    server
        .with_world(|world| {
            let mut ticker = world.resource_mut::<KeepAliveTicker>();
            ticker.interval_ticks = 3;
            ticker.ticks_since_last = 0;
        })
        .await;

    let mut previous = None;
    for _ in 0..3 {
        let id = alice
            .expect("a keep-alive", |packet| match packet {
                ClientboundGamePacket::KeepAlive(keep_alive) => Some(keep_alive.id),
                _ => None,
            })
            .await;
        assert!(previous < Some(id), "keep-alive ids increase");
        previous = Some(id);
    }
    server
        .wait_for("the last answer to be accepted", move |world| {
            let state = world.get::<KeepAliveState>(entity)?;
            (!state.awaiting_response).then_some(())
        })
        .await;

    alice.assert_healthy();
    server.stop().await;
}

/// A silent player whose keep-alive goes unanswered past the 30 s timeout is
/// disconnected with "Timed out", whether or not it ever reported loaded, and
/// its `max_players` slot is free again.
async fn silent_player_is_timed_out(report_loaded: bool) {
    let server = TestServer::builder()
        .config(|config| config.max_players(1))
        .start()
        .await;
    let mut dave = server.join_silent("Dave").await;
    let entity = server.player_entity(dave.uuid()).await;
    if report_loaded {
        dave.report_loaded().await;
        server
            .wait_for("Dave to be ready", move |world| {
                world.get::<PlayerReady>(entity).map(|_| ())
            })
            .await;
    }
    server
        .with_world(|world| {
            let mut ticker = world.resource_mut::<KeepAliveTicker>();
            ticker.interval_ticks = 3;
            ticker.ticks_since_last = 0;
        })
        .await;

    let id = dave
        .expect("a keep-alive", |packet| match packet {
            ClientboundGamePacket::KeepAlive(keep_alive) => Some(keep_alive.id),
            _ => None,
        })
        .await;
    // Dave never answers it. Backdating it past the 30 s timeout stands in
    // for waiting that long.
    server
        .with_world(move |world| {
            let mut state = world
                .get_mut::<KeepAliveState>(entity)
                .expect("Dave has a keep-alive state");
            assert!(state.awaiting_response, "Dave's keep-alive is pending");
            assert_eq!(state.last_sent_id as u64, id);
            state.last_sent_id -= 31_000;
        })
        .await;

    let reason = dave.expect_disconnect().await;
    assert_eq!(reason, "Timed out");
    server
        .wait_for("Dave's entity to be despawned", move |world| {
            world.get_entity(entity).is_err().then_some(())
        })
        .await;
    let erin = server
        .try_join("Erin")
        .await
        .unwrap_or_else(|refused| panic!("Erin was refused Dave's freed slot: {refused}"));

    erin.sync().await;
    erin.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn a_player_that_never_reports_loaded_and_stops_answering_is_timed_out() {
    silent_player_is_timed_out(false).await;
}

#[tokio::test]
async fn a_ready_player_that_stops_answering_is_timed_out() {
    silent_player_is_timed_out(true).await;
}

/// Shortened from vanilla's 30 s so the scenarios below can wait it out, yet
/// still several times longer than a join takes on a slow machine.
const PRE_PLAY_TIMEOUT: Duration = Duration::from_secs(5);

/// What vanilla shows for `multiplayer.disconnect.slow_login`.
const SLOW_LOGIN: &str = "Took too long to log in";

async fn start_with_short_pre_play_timeout(max_players: i32) -> TestServer {
    TestServer::builder()
        .config(|config| config.max_players(max_players))
        .plugin(|app| {
            app.insert_resource(PrePlayTimeout(PRE_PLAY_TIMEOUT));
        })
        .start()
        .await
}

async fn within<T>(what: &str, future: impl Future<Output = T>) -> T {
    let wait = PRE_PLAY_TIMEOUT + Duration::from_secs(15);
    tokio::time::timeout(wait, future)
        .await
        .unwrap_or_else(|_| panic!("timed out after {wait:?} waiting for {what}"))
}

/// A connection opened with the handshake for `intention`, nothing more.
async fn handshaken(
    address: SocketAddr,
    intention: ClientIntention,
) -> Connection<ClientboundHandshakePacket, ServerboundHandshakePacket> {
    let mut connection = Connection::new(&address)
        .await
        .unwrap_or_else(|error| panic!("could not connect to {address}: {error}"));
    connection
        .write(ServerboundIntention {
            protocol_version: PROTOCOL_VERSION,
            hostname: address.ip().to_string(),
            port: address.port(),
            intention,
        })
        .await
        .expect("handshake");
    connection
}

async fn wait_until_no_player(server: &TestServer) {
    server
        .wait_for("every player to be gone", |world| {
            let online = world.query::<&PlayerUuid>().iter(world).count();
            (online == 0).then_some(())
        })
        .await;
}

/// A client that opens a login and never says who it is is told, like
/// vanilla, that it took too long to log in, then closed.
#[tokio::test]
async fn a_client_stalled_in_login_is_told_it_took_too_long() {
    let server = start_with_short_pre_play_timeout(100).await;
    let started = Instant::now();
    let mut connection = handshaken(server.address(), ClientIntention::Login)
        .await
        .login();

    let packet = within("the login Disconnect", connection.read()).await;
    let Ok(ClientboundLoginPacket::LoginDisconnect(disconnect)) = packet else {
        panic!("expected a login Disconnect, got {packet:?}");
    };
    assert_eq!(disconnect.reason.to_string(), SLOW_LOGIN);
    assert!(
        started.elapsed() >= PRE_PLAY_TIMEOUT,
        "{:?}",
        started.elapsed()
    );
    let after = within("the connection to close", connection.read()).await;
    assert!(after.is_err(), "{after:?} after the Disconnect");

    server.stop().await;
}

/// A client that logged in but never finishes configuration is told it took
/// too long to log in, and its `max_players` slot is free again.
#[tokio::test]
async fn a_client_stalled_in_configuration_is_told_it_took_too_long() {
    let server = start_with_short_pre_play_timeout(1).await;
    let started = Instant::now();
    let carol = server
        .log_in("Carol")
        .await
        .unwrap_or_else(|refused| panic!("Carol was refused an empty server: {refused}"));

    let refused = carol.expect_disconnect().await;
    assert_eq!(
        refused.component["translate"], "multiplayer.disconnect.slow_login",
        "{refused}"
    );
    assert_eq!(refused.reason, SLOW_LOGIN);
    assert!(
        started.elapsed() >= PRE_PLAY_TIMEOUT,
        "{:?}",
        started.elapsed()
    );
    wait_until_no_player(&server).await;
    let erin = server
        .try_join("Erin")
        .await
        .unwrap_or_else(|refused| panic!("Erin was refused Carol's freed slot: {refused}"));

    erin.sync().await;
    erin.assert_healthy();
    server.stop().await;
}

/// A socket that sends nothing, and a server-list connection that never asks
/// for the status, have no disconnect packet: they are just closed.
#[tokio::test]
async fn a_connection_that_never_logs_in_is_closed() {
    let server = start_with_short_pre_play_timeout(100).await;
    let started = Instant::now();
    let mut idle = TcpStream::connect(server.address()).await.unwrap();
    let mut status = handshaken(server.address(), ClientIntention::Status)
        .await
        .status();

    let mut received = Vec::new();
    within(
        "the idle connection to close",
        idle.read_to_end(&mut received),
    )
    .await
    .expect("the server closes the idle connection cleanly");
    assert!(received.is_empty(), "nothing is sent before a handshake");
    let after = within("the status connection to close", status.read()).await;
    assert!(after.is_err(), "{after:?} on the status connection");
    assert!(
        started.elapsed() >= PRE_PLAY_TIMEOUT,
        "{:?}",
        started.elapsed()
    );

    server.stop().await;
}

/// A player who reached the play state in time is not affected by the
/// pre-play timeout running out afterwards.
#[tokio::test]
async fn a_player_who_joined_in_time_stays_past_the_pre_play_timeout() {
    let server = start_with_short_pre_play_timeout(100).await;
    let alice = server.join("Alice").await;

    tokio::time::sleep(PRE_PLAY_TIMEOUT + Duration::from_secs(1)).await;

    alice.sync().await;
    alice.assert_healthy();
    server.stop().await;
}

/// A player who sends a known packet whose body VoidMC cannot decode is
/// disconnected with the reason, like vanilla does; the server stays up and
/// the other players are not affected.
#[tokio::test]
async fn a_player_sending_a_malformed_packet_is_disconnected_with_the_reason() {
    let server = TestServer::start().await;
    let bob = server.join("Bob").await;

    for (index, (what, packet)) in malformed_play_packets().into_iter().enumerate() {
        let mallory = server.join(&format!("Mallory{index}")).await;
        let entity = server.player_entity(mallory.uuid()).await;
        mallory.sync().await;

        mallory.send_raw(&packet).await;
        let reason = mallory.expect_disconnect().await;
        assert!(reason.starts_with(DECODE_FAILURE), "{what}: {reason}");
        assert!(
            reason.contains(&format!("0x{:02X}", packet[0])),
            "{what}: {reason} names the packet id"
        );
        server
            .wait_for("the sender's entity to be despawned", move |world| {
                world.get_entity(entity).is_err().then_some(())
            })
            .await;
    }

    bob.sync().await;
    bob.assert_healthy();
    let erin = server.join("Erin").await;
    erin.sync().await;
    erin.assert_healthy();
    server.stop().await;
}

/// The same rule holds before the play state: a malformed Login Start is
/// answered with the login Disconnect, then the connection is closed.
#[tokio::test]
async fn a_malformed_login_packet_is_refused_with_the_reason() {
    let server = TestServer::start().await;
    let mut connection = handshaken(server.address(), ClientIntention::Login)
        .await
        .login();
    // A name, then a UUID cut short.
    let hello = raw(ServerboundHello {
        name: "Mallory".to_string(),
        profile_id: uuid::Uuid::nil(),
    });
    connection
        .writer
        .raw
        .write(&hello[..hello.len() - 8])
        .await
        .expect("send Login Start");

    let packet = within("the login Disconnect", connection.read()).await;
    let Ok(ClientboundLoginPacket::LoginDisconnect(disconnect)) = packet else {
        panic!("expected a login Disconnect, got {packet:?}");
    };
    let reason = disconnect.reason.to_string();
    assert!(reason.starts_with(DECODE_FAILURE), "{reason}");
    let after = within("the connection to close", connection.read()).await;
    assert!(after.is_err(), "{after:?} after the Disconnect");

    let alice = server.join("Alice").await;
    alice.sync().await;
    alice.assert_healthy();
    server.stop().await;
}

/// A malformed Client Information during configuration (a chat mode out of
/// range) is answered with the configuration Disconnect.
#[tokio::test]
async fn a_malformed_configuration_packet_is_refused_with_the_reason() {
    let server = TestServer::builder()
        .config(|config| config.max_players(1))
        .start()
        .await;
    let mut carol = server
        .log_in("Carol")
        .await
        .unwrap_or_else(|refused| panic!("Carol was refused an empty server: {refused}"));
    let mut information = raw(ServerboundClientInformation {
        information: ClientInformation::default(),
    });
    // The id, the locale (its length, then `en_us`), the view distance, then
    // the chat mode, 0 to 2.
    assert_eq!(&information[1..7], b"\x05en_us", "{information:?}");
    information[8] = 3;
    carol.send_raw(&information).await;

    let refused = carol.expect_disconnect().await;
    assert!(refused.reason.starts_with(DECODE_FAILURE), "{refused}");
    wait_until_no_player(&server).await;
    let erin = server
        .try_join("Erin")
        .await
        .unwrap_or_else(|refused| panic!("Erin was refused Carol's freed slot: {refused}"));
    erin.sync().await;
    erin.assert_healthy();
    server.stop().await;
}

/// A handshake cut short has no disconnect packet to answer it with: the
/// connection is closed at once, long before the pre-play timeout.
#[tokio::test]
async fn a_malformed_handshake_closes_the_connection() {
    let server = TestServer::start().await;
    let address = server.address();
    let intention = raw(ServerboundIntention {
        protocol_version: PROTOCOL_VERSION,
        hostname: address.ip().to_string(),
        port: address.port(),
        intention: ClientIntention::Login,
    });
    // Without its last field, the intention.
    let body = &intention[..intention.len() - 1];
    let mut frame = var_int(body.len() as u32);
    frame.extend(body);
    let mut stream = TcpStream::connect(address).await.unwrap();
    stream.write_all(&frame).await.unwrap();

    let mut received = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut received))
        .await
        .expect("the connection is closed well before the 30 s pre-play timeout")
        .expect("the server closes the connection cleanly");
    assert!(received.is_empty(), "{received:?}");

    let alice = server.join("Alice").await;
    alice.sync().await;
    alice.assert_healthy();
    server.stop().await;
}
