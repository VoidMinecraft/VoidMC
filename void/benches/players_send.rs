use std::hint::black_box;

use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemState;
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use voidmc::components::ClientId;
use voidmc::network::{ClientSenders, NetworkChannels, OutgoingPacket};
use voidmc::players::Players;
use voidmc_protocol::clientbound::KeepAlive;

const PACKETS: u64 = 1_024;

fn players_send(c: &mut Criterion) {
    let (_incoming_tx, incoming) = flume::unbounded();
    let (outgoing, _fallback) = flume::unbounded::<OutgoingPacket>();
    let (_disconnect_tx, disconnect) = flume::unbounded();
    let (kick, _kicked) = flume::unbounded();
    let (_connected_tx, connected) = flume::unbounded();
    let (direct, queue) = flume::bounded::<OutgoingPacket>(2 * PACKETS as usize);
    let mut world = World::new();
    world.insert_resource(NetworkChannels {
        incoming,
        outgoing,
        disconnect,
        kick,
    });
    let player = world.spawn(ClientId(1)).id();
    let mut senders = ClientSenders::new(connected);
    senders.register(player, direct);
    world.insert_resource(senders);
    let mut state = SystemState::<Players>::new(&mut world);

    let mut group = c.benchmark_group("players_send");
    group.throughput(Throughput::Elements(PACKETS));
    group.bench_function("direct_queue", |b| {
        b.iter(|| {
            let players = state.get(&world);
            for id in 0..PACKETS as i64 {
                players.send(player, KeepAlive { keep_alive_id: id });
            }
            for packet in queue.try_iter() {
                black_box(packet);
            }
        });
    });
    group.finish();
}

criterion_group!(benches, players_send);
criterion_main!(benches);
