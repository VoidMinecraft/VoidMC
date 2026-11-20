use voidmc::components::{KeepAliveState, PlayerReady};
use voidmc::systems::KeepAliveTicker;
use voidmc_e2e::TestServer;
use voidmc_e2e::protocol::packets::game::ClientboundGamePacket;

use crate::{feet, ground};

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
