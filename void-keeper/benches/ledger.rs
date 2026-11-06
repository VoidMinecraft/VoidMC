use std::hint::black_box;

use bytes::Bytes;
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use uuid::Uuid;
use voidmc_codec::{Encode, VarI32};
use voidmc_data::v26_1_2::packets::play::clientbound as cb;
use voidmc_keeper::ledger::Ledger;
use voidmc_keeper::wire::{frame, tagged};
use voidmc_protocol::clientbound::{Login, PlayPacket};

const PROFILE: Uuid = Uuid::from_u128(1);

fn login() -> Login {
    Login {
        entity_id: 1,
        is_hardcore: false,
        dimension_names: vec!["minecraft:overworld".to_string()],
        max_players: 10,
        view_distance: 10,
        simulation_distance: 10,
        reduced_debug_info: false,
        enable_respawn_screen: true,
        do_limited_crafting: false,
        dimension_type: 0,
        dimension_name: "minecraft:overworld".to_string(),
        hashed_seed: 0,
        game_mode: 1,
        previous_game_mode: -1,
        is_debug: false,
        is_flat: false,
        last_death_location: None,
        portal_cooldown: 0,
        sea_level: 63,
        enforces_secure_chat: false,
    }
}

fn add_entity(id: i32) -> Bytes {
    frame(cb::ADD_ENTITY, |body| VarI32(id).encode(body))
}

fn chunk(x: i32, z: i32) -> Bytes {
    frame(cb::LEVEL_CHUNK_WITH_LIGHT, |body| {
        x.encode(body);
        z.encode(body);
    })
}

fn full_ledger() -> Ledger {
    let mut ledger = Ledger::default();
    ledger.observe(&tagged(&PlayPacket::Login(login())));
    for id in 2..1_002 {
        ledger.observe(&add_entity(id));
    }
    for x in -16..16 {
        for z in -16..16 {
            ledger.observe(&chunk(x, z));
        }
    }
    ledger
}

fn observe(c: &mut Criterion) {
    let moved = frame(cb::MOVE_ENTITY_POS, |body| {
        VarI32(2).encode(body);
        body.extend_from_slice(&[0; 7]);
    });
    let mut ledger = full_ledger();
    c.bench_function("observe a packet that leaves no state", |b| {
        b.iter(|| ledger.observe(black_box(&moved)));
    });

    let spawned = add_entity(5_000);
    c.bench_function("observe a new entity", |b| {
        b.iter(|| ledger.observe(black_box(&spawned)));
    });

    let loaded = chunk(40, 40);
    c.bench_function("observe a chunk", |b| {
        b.iter(|| ledger.observe(black_box(&loaded)));
    });
}

fn teardown(c: &mut Criterion) {
    let login = login();
    c.bench_function("teardown of 1000 entities and 1024 chunks", |b| {
        b.iter_batched(
            full_ledger,
            |mut ledger| ledger.teardown(black_box(&login), PROFILE),
            BatchSize::LargeInput,
        );
    });
}

criterion_group!(benches, observe, teardown);
criterion_main!(benches);
