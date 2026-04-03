# Benchmarking & Profiling

Deep dive into measuring and optimizing Void's performance.

## Benchmark Tools

### Cargo Bench

Built-in benchmarking framework:

```bash
# Run all benchmarks
cargo bench

# Run specific benchmark
cargo bench -- bench_name

# Save baseline for comparison
cargo bench -- --save-baseline baseline1

# Compare against baseline
cargo bench -- --baseline baseline1
```

**Example benchmark:**

```rust
#![feature(test)]
extern crate test;

use test::Bencher;

#[bench]
fn bench_packet_encode(b: &mut Bencher) {
    let packet = SpawnEntity { /* ... */ };
    b.iter(|| {
        let mut buf = Vec::new();
        packet.encode(&mut buf);
    })
}
```

### Criterion.rs

More advanced benchmarking library (requires setup):

```bash
cargo install criterion
```

### Flame Graphs

Visualize CPU time distribution:

```bash
# Install
cargo install flamegraph

# Generate
cargo flamegraph

# View
open flamegraph.svg
```

## Profiling Tools

### Linux: Perf

System-level profiling:

```bash
# Record
sudo perf record -F 99 cargo run --release

# Report
sudo perf report

# Generate flame graph
sudo perf script > /tmp/perf.out
~/FlameGraph/stackcollapse-perf.pl /tmp/perf.out > /tmp/perf.folded
~/FlameGraph/flamegraph.pl /tmp/perf.folded > perf.svg
```

### macOS: Instruments

GUI profiling tool:

```bash
# Time profiler
instruments -t 'Time Profiler' target/release/void

# Allocations
instruments -t 'Allocations' target/release/void
```

### Memory Profiling

```bash
# DHAT (Linux)
cargo install valgrind
valgrind --tool=dhat target/release/void

# Heaptrack (Linux)
heaptrack target/release/void
heaptrack_gui heaptrack.out.2024…
```

## Performance Testing Scenarios

### Scenario 1: Idle Server

Measure baseline resource usage:

```rust
#[tokio::test]
async fn measure_idle_memory() {
    let server = Server::new().await;

    // Wait and measure
    tokio::time::sleep(Duration::from_secs(10)).await;

    let memory = get_current_memory_usage();
    println!("Idle memory: {} MB", memory);
}
```

### Scenario 2: Rapid Connections

Measure connection acceptance performance:

```rust
#[tokio::test]
async fn measure_connection_rate() {
    let server = Server::new().await;
    let start = Instant::now();

    let mut handles = vec![];
    for _ in 0..1000 {
        handles.push(tokio::spawn(async {
            TcpStream::connect(server.addr()).await
        }));
    }

    futures::future::join_all(handles).await;
    let elapsed = start.elapsed();

    let rate = 1000.0 / elapsed.as_secs_f64();
    println!("Connections/sec: {}", rate);
}
```

### Scenario 3: Sustained Load

Measure under constant traffic:

```rust
#[tokio::test]
async fn sustained_load_test() {
    let server = Server::new().await;
    let start = Instant::now();

    // Spawn 100 virtual clients
    let clients: Vec<_> = (0..100)
        .map(|_| {
            let addr = server.addr();
            tokio::spawn(async move {
                let mut conn = TcpStream::connect(addr).await.unwrap();

                // Send packets for 60 seconds
                let deadline = Instant::now() + Duration::from_secs(60);
                while Instant::now() < deadline {
                    // Send packet and measure latency
                }
            })
        })
        .collect();

    futures::future::join_all(clients).await;
    let elapsed = start.elapsed();

    println!("Test completed in: {:?}", elapsed);
}
```

## Analyzing Results

### Identifying Bottlenecks

1. **Look at hot functions**: Functions taking most time
2. **Check allocations**: High allocation = potential speedup
3. **Review I/O**: Network and disk operations
4. **Inspect algorithms**: O(n²) where O(n) possible?

### Creating Benchmarks

Before optimization:

```bash
cargo bench --bench '*' -- --save-baseline before
```

After optimization:

```bash
cargo bench --bench '*' -- --baseline before
```

Compare results automatically.

### Regression Prevention

- Keep baseline benchmarks in CI/CD
- Alert if performance degrades > 5%
- Document why when optimizations trade off with readability

## Performance Checklist

- [ ] Measured baseline performance
- [ ] Identified bottlenecks with profiling
- [ ] Optimized critical paths
- [ ] Verified improvements with benchmarks
- [ ] No performance regressions introduced
- [ ] Code clarity maintained
- [ ] Results documented

---

Remember: **Profile before optimizing, and verify improvements after.**
