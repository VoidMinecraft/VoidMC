#![cfg(feature = "packet-trace")]

use std::panic::{AssertUnwindSafe, catch_unwind};

use bevy_app::{App, Update};
use bevy_ecs::prelude::*;
use voidmc::components::ClientId;
use voidmc::config::{ServerConfigBuilder, ServerConfigResource};
use voidmc::network::NetworkPlugin;
use voidmc::packet_trace::PacketTracePlugin;
use voidmc::players::Players;
use voidmc_protocol::clientbound::KeepAlive;

fn send_then_die(players: Players, clients: Query<Entity, With<ClientId>>) {
    for client in &clients {
        players.send(client, KeepAlive { keep_alive_id: 7 });
    }
    panic!("the tick dies");
}

#[test]
fn the_lines_of_the_tick_that_panics_reach_the_file() {
    let path = std::env::temp_dir().join(format!("void-panic-{}.jsonl", std::process::id()));
    // SAFETY: this file holds one test, and nothing else runs yet.
    unsafe { std::env::set_var("VOID_TRACE", &path) };

    let (_incoming_tx, incoming_rx) = flume::unbounded();
    let (outgoing_tx, _outgoing_rx) = flume::unbounded();
    let (_disconnect_tx, disconnect_rx) = flume::unbounded();
    let (kick_tx, _kick_rx) = flume::unbounded();
    let (_connected_tx, connected_rx) = flume::unbounded();
    let mut app = App::new();
    app.add_plugins(NetworkPlugin::new(
        incoming_rx,
        outgoing_tx,
        disconnect_rx,
        kick_tx,
        connected_rx,
    ))
    .add_plugins(PacketTracePlugin)
    .insert_resource(ServerConfigResource::from(
        &ServerConfigBuilder::new().build(),
    ))
    .add_systems(Update, send_then_die);
    app.world_mut().spawn(ClientId(3));

    assert!(catch_unwind(AssertUnwindSafe(|| app.update())).is_err());

    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(text.lines().count(), 1, "{text}");
    assert!(text.contains("Play/KeepAlive"), "{text}");
}
