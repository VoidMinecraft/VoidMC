use std::collections::VecDeque;
use std::hint::{black_box, spin_loop};
use std::sync::{Arc, Mutex};
use std::thread;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

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
}

criterion_group!(benches, channel_handoff);
criterion_main!(benches);
