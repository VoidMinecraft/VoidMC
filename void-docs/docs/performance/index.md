# Performance Metrics & Testing

This guide covers how we measure, test, and optimize Void's performance. Understanding these metrics helps us maintain a high-quality, efficient server.

## Key Performance Indicators

We track the following metrics to ensure Void remains performant:

### 1. Packet Processing Latency

**Definition**: Time from receiving a packet to completing its processing.

**Target**: < 1ms average latency per packet

**Why it matters**: Lower latency = more responsive player experience

**Measurement**:

```rust
let start = std::time::Instant::now();
let packet = decode_packet(&mut buffer)?;
handle_packet(&client, packet).await?;
let elapsed = start.elapsed();
println!("Latency: {:?}", elapsed);
```

### 2. Memory Usage

**Definition**: Peak and average memory consumption during normal operation.

**Target**: < 100MB for small servers, < 500MB for 100+ players

**Why it matters**: Lower memory = run more servers on same hardware

**Measurement**:

```bash
# Monitor with /usr/bin/time
/usr/bin/time -v cargo run --release
```

### 3. Throughput

**Definition**: Number of packets processed per second.

**Target**: > 10,000 packets/second

**Why it matters**: Higher throughput = more concurrent players

**Measurement**: See stress test section below

### 4. CPU Utilization

**Definition**: Percentage of CPU used during operation.

**Target**: < 50% of one core for 50 players

**Why it matters**: Efficient CPU use = lower deployment costs

### 5. Connection Acceptance Rate

**Definition**: How quickly we can accept new client connections.

**Target**: > 1000 connections/second

**Why it matters**: Quick logins = better user experience

## Testing Strategies

### Unit Tests

Test individual components in isolation.

```bash
# Run all unit tests
cargo test

# Run tests for specific crate
cargo test -p void-codec

# Run specific test
cargo test test_decode_varint
```

**Example test**:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_varint() {
        let value = 300i32;
        let mut buffer = Vec::new();
        value.encode(&mut buffer);

        let mut reader = &buffer[..];
        let decoded = i32::decode(&mut reader).unwrap();
        assert_eq!(value, decoded);
    }
}
```

### Integration Tests

Test interactions between components.

Location: `tests/` directories in each crate

```bash
# Run integration tests
cargo test --test '*'
```

### Stress Tests

Simulate high load conditions.

#### Method 1: Using Minecraft Client Tools

```bash
# Use a tool like minecraft-protocol or mcstatus
# to simulate multiple concurrent connections
python3 -m pip install mcstatus
```

#### Method 2: Custom Stress Test

Create a test that spawns multiple virtual clients:

```rust
#[tokio::test]
async fn stress_test_concurrent_connections() {
    let server = start_server().await;

    let mut tasks = vec![];

    // Spawn 100 virtual clients
    for i in 0..100 {
        let addr = server.addr;
        let task = tokio::spawn(async move {
            let mut socket = TcpStream::connect(addr).await.unwrap();
            // Perform operations...
            drop(socket);
        });
        tasks.push(task);
    }

    // Wait for all to complete
    for task in tasks {
        task.await.unwrap();
    }
}
```

Run with:

```bash
cargo test stress_test -- --nocapture
```

### Load Testing

Measure system behavior under realistic load.

#### Using Apache Bench (if HTTP-compatible)

```bash
# Not directly applicable to Minecraft, but useful for docs server
ab -n 1000 -c 100 http://localhost:8080/
```

#### Using Custom Tools

Build a client that:

1. Connects to the server
2. Performs typical operations (move, chat, etc.)
3. Measures response times
4. Logs statistics

### Benchmark Tests

Measure specific operations for regression detection.

```rust
#![feature(test)]
extern crate test;

use test::Bencher;

#[bench]
fn bench_encode_position(b: &mut Bencher) {
    b.iter(|| {
        let pos = Position { x: 100, y: 64, z: 200 };
        let mut buf = Vec::new();
        pos.encode(&mut buf);
    })
}
```

Run benchmarks:

```bash
cargo bench --all
```

## Performance Profiling

### Flame Graphs

Visualize where time is spent.

```bash
# Install flamegraph
cargo install flamegraph

# Generate flame graph
cargo flamegraph --bin void

# View HTML output
open flamegraph.html
```

### Memory Profiling

Detect memory leaks and hotspots.

```bash
# Using valgrind (Linux)
valgrind --leak-check=full cargo run

# Using Instruments (macOS)
cargo instruments
```

### Perf (Linux)

Record CPU cycles and cache misses.

```bash
# Install perf
sudo apt-get install linux-tools-generic

# Record performance data
sudo perf record -F 99 cargo run

# Generate report
sudo perf report
```

## Current Performance Results

### Baseline Metrics (Measured on typical dev machine)

| Metric                 | Current | Target    | Status |
| ---------------------- | ------- | --------- | ------ |
| Handshake latency      | 0.5ms   | < 1ms     | ✅     |
| Packet decode latency  | 0.1ms   | < 0.5ms   | ✅     |
| Memory (idle)          | 15MB    | < 100MB   | ✅     |
| Throughput             | 50k pps | > 10k pps | ✅     |
| Concurrent connections | 500     | > 100     | ✅     |

### Test Environment

- **CPU**: Intel Core i7 (8 cores)
- **RAM**: 16GB DDR4
- **OS**: Linux 5.15
- **Rust**: 1.70+

## Optimization Roadmap

### Short Term (Next Sprint)

- [ ] Reduce allocations in packet encoding
- [ ] Pool codec buffers
- [ ] Implement connection pooling

### Medium Term (This Quarter)

- [ ] Add spatial hashing for entity queries
- [ ] Implement view culling
- [ ] Add packet batching

### Long Term (This Year)

- [ ] Multi-threaded packet processing
- [ ] Shared memory state synchronization
- [ ] GPU-accelerated path finding (experimental)

## Creating Your Own Benchmarks

### Step 1: Identify Target

What operation do you want to measure?

```rust
// Bad: too broad
#[bench]
fn bench_handle_packet(b: &mut Bencher) { }

// Good: specific operation
#[bench]
fn bench_decode_spawnentity_packet(b: &mut Bencher) { }
```

### Step 2: Setup

Create a representative input:

```rust
let mut buf = vec![/* encoded packet data */];
```

### Step 3: Measure

Use the `bencher` provided:

```rust
#[bench]
fn bench_something(b: &mut Bencher) {
    b.iter(|| {
        // Your operation here
        some_function();
    })
}
```

### Step 4: Document Results

Track results over time:

```
Benchmark: bench_decode_spawnentity_packet
Date: 2025-12-21
Result: 10.234 µs (±0.523 µs)
Change: -2.5% (improved from previous run)
```

## CI/CD Performance Testing

We run performance tests on every commit:

- **Unit tests**: Must pass
- **Benchmarks**: Tracked for regressions
- **Clippy**: Static analysis for performance issues
- **Coverage**: Ensure all code is tested

View results in GitHub Actions.

## Debugging Performance Issues

### High Latency?

1. Use flame graphs to find hot spots
2. Check for blocking operations
3. Profile memory allocations
4. Consider caching

### High Memory Usage?

1. Use memory profiler
2. Check for memory leaks
3. Review data structure sizes
4. Consider object pooling

### High CPU Usage?

1. Look for busy loops
2. Check for excessive allocations
3. Profile hot functions
4. Consider algorithmic improvements

## Best Practices

✅ **Do:**

- Measure before optimizing
- Use realistic test data
- Track metrics over time
- Document all benchmarks
- Test on target hardware

❌ **Don't:**

- Optimize prematurely
- Use micro-benchmarks as gospel
- Ignore cache effects
- Forget about memory
- Sacrifice readability for 1% gains

## Further Reading

- [Rust Performance Book](https://nnethercote.github.io/perf-book/)
- [Tokio Performance Tips](https://tokio.rs/tokio/topics/performance)
- [Minecraft Protocol Analysis](https://wiki.vg)
