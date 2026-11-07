#![cfg(feature = "packet-trace")]

//! These read the spans bevy opens around real systems. Those spans share one callsite, whose
//! interest `tracing` caches the first time any thread reaches it: a test thread without a
//! subscriber can cache "never" for the whole binary. So this runs in a process of its own,
//! under one global subscriber, as a server does.

use bevy_app::{App, Update};
use bevy_ecs::prelude::*;
use serde_json::Value;
use tracing_subscriber::layer::SubscriberExt;
use voidmc::components::ClientId;
use voidmc::config::{ServerConfigBuilder, ServerConfigResource};
use voidmc::network::NetworkPlugin;
use voidmc::packet_trace::{PacketTracePlugin, trace_layer};
use voidmc::players::{Players, WorldPlayers};
use voidmc_protocol::clientbound::KeepAlive;

#[derive(Event)]
struct Poke;

fn send(world: &World, keep_alive_id: i64) {
    let client = world
        .try_query_filtered::<Entity, With<ClientId>>()
        .and_then(|mut clients| clients.iter(world).next());
    WorldPlayers::new(world).send(client.unwrap(), KeepAlive { keep_alive_id });
}

fn plain(players: Players, clients: Query<Entity, With<ClientId>>) {
    players.send(clients.single().unwrap(), KeepAlive { keep_alive_id: 1 });
}

fn poke(world: &mut World) {
    send(world, 2);
    world.trigger(Poke);
    send(world, 5);
}

fn observer(_: On<Poke>, players: Players, clients: Query<Entity, With<ClientId>>) {
    players.send(clients.single().unwrap(), KeepAlive { keep_alive_id: 3 });
}

fn deferring_observer(_: On<Poke>, mut commands: Commands) {
    commands.queue(|world: &mut World| send(world, 4));
}

fn deferring_system(mut commands: Commands) {
    commands.queue(|world: &mut World| send(world, 6));
}

fn system_of(line: &Value) -> &str {
    let name = line["system"].as_str().unwrap_or("(none)");
    name.rsplit("::").next().unwrap_or(name)
}

#[test]
fn every_line_names_the_system_that_was_running() {
    let path = std::env::temp_dir().join(format!("void-systems-{}.jsonl", std::process::id()));
    // SAFETY: this file holds one test, and nothing else runs yet.
    unsafe { std::env::set_var("VOID_TRACE", &path) };
    let subscriber = tracing_subscriber::registry().with(trace_layer());
    tracing::subscriber::set_global_default(subscriber).unwrap();

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
    .add_observer(observer)
    .add_observer(deferring_observer)
    .add_systems(Update, (plain, poke, deferring_system).chain());
    app.world_mut().spawn(ClientId(3));

    app.update();
    send(app.world_mut(), 7);
    app.update();

    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let lines: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let systems: Vec<&str> = lines.iter().map(system_of).collect();
    let tick = [
        "plain",
        "poke",
        "observer",
        "poke",
        "poke",
        "deferring_system",
    ];
    assert_eq!(systems[..6], tick, "{text}");
    assert_eq!(systems[6], "(none)", "between two ticks, {text}");
    assert_eq!(systems[7..13], tick, "the second tick, {text}");
}
