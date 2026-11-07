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
use voidmc_keeper::frame::{self, FrameCodec};
use voidmc_keeper::proxy::{Events, Relay};
use voidmc_keeper::session::{Session, Settings};

const READ_CHUNK: usize = 64 * 1024;
const PAPER_THRESHOLD: i32 = 256;

fn empty(id: i32) -> Bytes {
    frame::encode(id, |_| {})
}

fn wire(codec: FrameCodec, frames: &[Bytes]) -> Vec<u8> {
    let mut out = Vec::new();
    for frame in frames {
        codec.encode(frame, &mut out);
    }
    out
}

/// A relay in play, behind a backend that compresses from `threshold` on, if it does.
fn in_play(threshold: Option<i32>) -> (Relay<()>, FrameCodec) {
    let session = Session::new(Settings::default(), Instant::now(), empty(0));
    let mut relay = Relay::new(1, session, (), Events::new(|_| {}));
    relay.backend_connected(());
    let plain = FrameCodec::default();
    let mut backend = FrameCodec::default();
    let hello = frame::encode(login_sb::HELLO, |body| {
        "Bot".to_string().encode(body);
        Uuid::from_u128(1).encode(body);
    });
    relay.received_from_client(&wire(plain, &[hello])).unwrap();
    if let Some(threshold) = threshold {
        let compression = frame::encode(login_cb::LOGIN_COMPRESSION, |body| {
            VarI32(threshold).encode(body)
        });
        relay.received_from_backend(&wire(plain, &[compression]));
        backend.set_threshold(threshold);
    }
    let finished = frame::encode(login_cb::LOGIN_FINISHED, |body| {
        Uuid::from_u128(1).encode(body)
    });
    relay.received_from_backend(&wire(backend, &[finished]));
    relay
        .received_from_client(&wire(plain, &[empty(login_sb::LOGIN_ACKNOWLEDGED)]))
        .unwrap();
    relay.received_from_backend(&wire(backend, &[empty(config_cb::FINISH_CONFIGURATION)]));
    relay
        .received_from_client(&wire(plain, &[empty(config_sb::FINISH_CONFIGURATION)]))
        .unwrap();
    relay.discard_queued();
    (relay, backend)
}

/// Nine small movements for one chunk of 10 KB, as a server streams play.
fn play_frames() -> Vec<Bytes> {
    let small = frame::encode(play_cb::MOVE_ENTITY_POS, |body| {
        VarI32(42).encode(body);
        body.extend_from_slice(&[0; 7]);
    });
    let chunk = frame::encode(play_cb::LEVEL_CHUNK_WITH_LIGHT, |body| {
        3i32.encode(body);
        4i32.encode(body);
        body.extend((0..10_000u32).map(|i| (i * 31 % 251 % 16) as u8));
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
    let frames = play_frames();
    let mut group = c.benchmark_group("relay");
    group.throughput(Throughput::Elements(frames.len() as u64));
    for (name, threshold) in [
        ("play_from_backend", None),
        ("play_from_backend_compressed", Some(PAPER_THRESHOLD)),
    ] {
        let (mut relay, backend) = in_play(threshold);
        let bytes = wire(backend, &frames);
        relay.received_from_backend(&bytes);
        assert!(
            relay.discard_queued() > bytes.len() / 2,
            "{name} relays nothing"
        );
        group.bench_function(name, |b| {
            b.iter(|| {
                for read in bytes.chunks(READ_CHUNK) {
                    relay.received_from_backend(read);
                    black_box(relay.discard_queued());
                }
            })
        });
    }
    group.finish();
}

criterion_group!(benches, relay);
criterion_main!(benches);
