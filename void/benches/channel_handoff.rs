use std::collections::VecDeque;
use std::hint::{black_box, spin_loop};
use std::sync::{Arc, Mutex};
use std::thread;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use voidmc_codec::Encode;
use voidmc_protocol::clientbound::{ChunkDataAndLight, ChunkHeightmaps};

const MESSAGE_COUNT: u64 = 1_024;

fn flume_handoff(message_count: u64) {
    let (tx, rx) = flume::unbounded();
    let receiver = thread::spawn(move || {
        for _ in 0..message_count {
            black_box(rx.recv().unwrap());
        }
    });

    for id in 0..message_count {
        tx.send(id).unwrap();
    }

    receiver.join().unwrap();
}

fn mutex_vecdeque_handoff(message_count: u64) {
    let queue = Arc::new(Mutex::new(VecDeque::with_capacity(message_count as usize)));
    let receiver_queue = Arc::clone(&queue);

    let receiver = thread::spawn(move || {
        let mut received = 0;
        while received < message_count {
            let next = receiver_queue.lock().unwrap().pop_front();
            if let Some(value) = next {
                black_box(value);
                received += 1;
            } else {
                spin_loop();
            }
        }
    });

    for id in 0..message_count {
        queue.lock().unwrap().push_back(id);
    }

    receiver.join().unwrap();
}

fn bounded_flume_payload(message_count: u64, bytes: usize, recipients: usize) {
    let payload: Arc<[u8]> = vec![0; bytes].into();
    let mut senders = Vec::new();
    let mut readers = Vec::new();
    for _ in 0..recipients {
        let (tx, rx) = flume::bounded::<Arc<[u8]>>(32);
        senders.push(tx);
        readers.push(thread::spawn(move || {
            for _ in 0..message_count {
                black_box(rx.recv().unwrap());
            }
        }));
    }
    for _ in 0..message_count {
        for tx in &senders {
            tx.send(Arc::clone(&payload)).unwrap();
        }
    }
    for reader in readers {
        reader.join().unwrap();
    }
}

fn bounded_tokio_payload(message_count: u64, bytes: usize, recipients: usize) {
    let payload: Arc<[u8]> = vec![0; bytes].into();
    let mut senders = Vec::new();
    let mut readers = Vec::new();
    for _ in 0..recipients {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Arc<[u8]>>(32);
        senders.push(tx);
        readers.push(thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                for _ in 0..message_count {
                    black_box(rx.recv().await.unwrap());
                }
            });
        }));
    }
    for _ in 0..message_count {
        for tx in &senders {
            tx.blocking_send(Arc::clone(&payload)).unwrap();
        }
    }
    for reader in readers {
        reader.join().unwrap();
    }
}

fn chunk(bytes: usize) -> ChunkDataAndLight {
    ChunkDataAndLight {
        chunk_x: 0,
        chunk_z: 0,
        heightmaps: ChunkHeightmaps::empty(),
        data: vec![0; bytes],
        block_entities: vec![],
        sky_light_mask: vec![],
        block_light_mask: vec![],
        empty_sky_light_mask: vec![],
        empty_block_light_mask: vec![],
        sky_light_arrays: vec![],
        block_light_arrays: vec![],
    }
}

fn encode_chunk(chunk: &ChunkDataAndLight, passes: usize) {
    let mut encoded = Vec::new();
    for _ in 0..passes {
        encoded.clear();
        chunk.encode(&mut encoded);
        black_box(encoded.len());
    }
}

fn channel_handoff(c: &mut Criterion) {
    let mut group = c.benchmark_group("channel_handoff");
    group.throughput(Throughput::Elements(MESSAGE_COUNT));

    group.bench_function(BenchmarkId::new("handoff", "flume_unbounded"), |b| {
        b.iter(|| flume_handoff(black_box(MESSAGE_COUNT)));
    });

    group.bench_function(
        BenchmarkId::new("handoff", "mutex_vecdeque_prototype"),
        |b| {
            b.iter(|| mutex_vecdeque_handoff(black_box(MESSAGE_COUNT)));
        },
    );

    group.finish();

    let mut bounded = c.benchmark_group("bounded_payload_handoff");
    for (bytes, recipients) in [(64, 1), (64 * 1024, 1), (64, 16), (64 * 1024, 16)] {
        let name = format!("{bytes}b_to_{recipients}");
        bounded.throughput(Throughput::Elements(MESSAGE_COUNT * recipients as u64));
        bounded.bench_function(BenchmarkId::new("flume", &name), |b| {
            b.iter(|| bounded_flume_payload(black_box(MESSAGE_COUNT), bytes, recipients));
        });
        bounded.bench_function(BenchmarkId::new("tokio_mpsc", &name), |b| {
            b.iter(|| bounded_tokio_payload(black_box(MESSAGE_COUNT), bytes, recipients));
        });
    }
    bounded.finish();

    let mut serialization = c.benchmark_group("outbound_chunk_accounting");
    for bytes in [64 * 1024, 1024 * 1024] {
        let chunk = chunk(bytes);
        serialization.throughput(Throughput::Bytes(bytes as u64));
        for passes in [1, 2] {
            serialization.bench_function(
                BenchmarkId::new(format!("{passes}_encode_passes"), bytes),
                |b| {
                    b.iter(|| encode_chunk(black_box(&chunk), passes));
                },
            );
        }
    }
    serialization.finish();
}

criterion_group!(benches, channel_handoff);
criterion_main!(benches);
