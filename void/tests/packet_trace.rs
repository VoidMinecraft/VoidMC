#![cfg(feature = "packet-trace")]

use bevy_app::{App, Update};
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ExecutorKind;
use tracing_subscriber::layer::SubscriberExt;
use voidmc::components::ClientId;
use voidmc::config::{ServerConfigBuilder, ServerConfigResource};
use voidmc::network::{ClientSenders, NetworkPlugin};
use voidmc::packet_trace::{PacketTracePlugin, trace_layer};
use voidmc::players::Players;
use voidmc_protocol::clientbound::KeepAlive;

fn keep_alive(players: Players, clients: Query<Entity, With<ClientId>>) {
    for client in &clients {
        players.send(client, KeepAlive { keep_alive_id: 7 });
    }
}

#[test]
fn a_packet_sent_by_a_system_lands_in_the_trace_with_its_tick_and_system() {
    let path = std::env::temp_dir().join(format!("void-trace-{}.jsonl", std::process::id()));
    // SAFETY: this file holds one test, and nothing else runs yet.
    unsafe { std::env::set_var("VOID_TRACE", &path) };

    let subscriber = tracing_subscriber::registry().with(trace_layer());
    tracing::subscriber::with_default(subscriber, || {
        let (_incoming_tx, incoming_rx) = flume::unbounded();
        let (outgoing_tx, outgoing_rx) = flume::unbounded();
        let (_disconnect_tx, disconnect_rx) = flume::unbounded();
        let (kick_tx, _kick_rx) = flume::unbounded();
        let (_connected_tx, connected_rx) = flume::unbounded();

        let mut app = App::new();
        app.edit_schedule(Update, |schedule| {
            schedule.set_executor_kind(ExecutorKind::SingleThreaded);
        });
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
        .add_systems(Update, keep_alive);
        app.world_mut().spawn(ClientId(3));

        app.update();
        assert_eq!(outgoing_rx.len(), 1, "tracing must not swallow the packet");
        app.update();

        let (stalled_tx, _stalled_rx) = flume::bounded(1);
        let (gone_tx, gone_rx) = flume::bounded(1);
        drop(gone_rx);
        let stalled = app.world_mut().spawn(ClientId(5)).id();
        let gone = app.world_mut().spawn(ClientId(6)).id();
        let mut senders = app.world_mut().resource_mut::<ClientSenders>();
        senders.register(stalled, stalled_tx);
        senders.register(gone, gone_tx);
        for _ in 0..3 {
            app.update();
        }
    });

    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let lines: Vec<serde_json::Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let of = |client: u32| {
        let lines = lines.iter().filter(|line| line["client"] == client);
        lines.collect::<Vec<_>>()
    };
    let fates = |client| {
        let fates = of(client).into_iter().map(|line| line["dropped"].as_str());
        fates.collect::<Vec<_>>()
    };
    let first = voidmc::server_tick::current() - 4;
    assert_eq!(of(3)[0]["tick"], first);
    assert_eq!(of(3)[1]["tick"], first + 1);
    assert_eq!(fates(3), [None, None, None, None, None]);
    assert_eq!(fates(5), [None, Some("full"), Some("kicked")]);
    assert_eq!(fates(6), [Some("closed"); 3]);
    assert_eq!(lines[0]["dir"], "out");
    assert_eq!(lines[0]["client"], 3);
    assert_eq!(lines[0]["packet"], "Play/KeepAlive");
    assert_eq!(lines[0]["bytes"], 9);
    let system = lines[0]["system"].as_str().unwrap();
    assert!(system.ends_with("keep_alive"), "{system}");
}
