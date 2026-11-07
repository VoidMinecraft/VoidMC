use std::hint::black_box;
use std::time::Instant;

use bytes::Bytes;
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use uuid::Uuid;
use voidmc_codec::{Encode, VarI32};
use voidmc_data::v26_1_2::packets::configuration::{
    clientbound as config_cb, serverbound as config_sb,
};
use voidmc_data::v26_1_2::packets::login::{clientbound as login_cb, serverbound as login_sb};
use voidmc_data::v26_1_2::packets::play::clientbound as play_cb;
use voidmc_keeper::session::{Session, Settings};
use voidmc_keeper::wire::frame;

fn empty(id: i32) -> Bytes {
    frame(id, |_| {})
}

fn in_play(now: Instant) -> Session {
    let mut session = Session::new(Settings::default(), now, empty(0));
    session.on_tick(now);
    session.on_backend_connected(now);
    session.on_client(
        frame(login_sb::HELLO, |body| {
            "Bot".to_string().encode(body);
            Uuid::from_u128(1).encode(body);
        }),
        now,
    );
    session.on_backend(
        frame(login_cb::LOGIN_FINISHED, |body| {
            Uuid::from_u128(1).encode(body)
        }),
        now,
    );
    session.on_client(empty(login_sb::LOGIN_ACKNOWLEDGED), now);
    session.on_backend(empty(config_cb::FINISH_CONFIGURATION), now);
    session.on_client(empty(config_sb::FINISH_CONFIGURATION), now);
    session.take_actions();
    session
}

/// Nine small movements for one chunk of 10 KB, as a server streams play.
fn play_frames() -> Vec<Bytes> {
    let small = frame(play_cb::MOVE_ENTITY_POS, |body| {
        VarI32(42).encode(body);
        body.extend_from_slice(&[0; 7]);
    });
    let chunk = frame(play_cb::LEVEL_CHUNK_WITH_LIGHT, |body| {
        3i32.encode(body);
        4i32.encode(body);
        body.resize(10_000, 0);
    });
    (0..1000)
        .map(|i| {
            if i % 10 == 0 {
                chunk.clone()
            } else {
                small.clone()
            }
        })
        .collect()
}

fn relay(c: &mut Criterion) {
    let now = Instant::now();
    let mut session = in_play(now);
    let frames = play_frames();
    let mut group = c.benchmark_group("relay");
    group.throughput(Throughput::Elements(frames.len() as u64));
    group.bench_function("play_from_backend", |b| {
        b.iter(|| {
            for frame in &frames {
                session.on_backend(frame.clone(), now);
            }
            black_box(session.take_actions());
        })
    });
    group.finish();
}

criterion_group!(benches, relay);
criterion_main!(benches);
