use voidmc::components::KeepAliveState;
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
