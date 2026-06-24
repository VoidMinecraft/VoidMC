# Benchmarking & Profiling

This page documents the exact commands used to collect performance evidence.

## Correctness Before Performance

Run correctness checks before trusting benchmark results:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

For the standalone POC crate:

```bash
cargo fmt --manifest-path pocs/performance/Cargo.toml -- --check
cargo clippy --manifest-path pocs/performance/Cargo.toml --all-targets -- -D warnings
```

## Criterion Benchmarks

Codec comparison:

```bash
cargo bench -p voidmc-codec --bench codec_comparison
```

Network-to-ECS handoff comparison:

```bash
cargo bench -p voidmc --bench channel_handoff
```

Chunk and representative packet serialization:

```bash
cargo bench -p voidmc-protocol --bench packet_chunk
```

Save a local baseline when preparing an optimization:

```bash
cargo bench -p voidmc-protocol --bench packet_chunk -- --save-baseline before
```

Compare after the change:

```bash
cargo bench -p voidmc-protocol --bench packet_chunk -- --baseline before
```

Criterion writes detailed reports under `target/criterion/`. Do not commit that directory; summarize the relevant median estimates in [Metrics & Results](./metrics-results.md).

## TPS Collection

Run the example server in release mode with metrics enabled:

```bash
VOID_METRICS_DEBUG=1 VOID_TPS_OUTPUT=logs/tps-demo.csv cargo run --release -p voidmc-example
```

The CSV columns are:

| Column | Meaning |
| --- | --- |
| `timestamp_ms` | Unix timestamp in milliseconds |
| `tps` | Ticks per second over the metrics window |
| `window_ms` | Actual elapsed measurement window |
| `last_tick_ms` | Duration of the latest completed tick |
| `total_ticks` | Total ticks since startup |

For a short local sample without committing generated files:

```bash
rm -f /tmp/voidmc-tps-demo.csv
timeout 45s env VOID_METRICS_DEBUG=1 VOID_TPS_OUTPUT=/tmp/voidmc-tps-demo.csv cargo run --release -p voidmc-example || test $? -eq 124
head /tmp/voidmc-tps-demo.csv
```

## TCP Connect Stress

Start the server:

```bash
cargo run --release -p voidmc-example
```

Run the POC from another shell:

```bash
cargo run --manifest-path pocs/performance/Cargo.toml --release --bin tcp_connect_stress -- --addr 127.0.0.1:25565 --clients 64 --timeout-ms 1000
```

The tool reports connection attempts, successful connects, failures, timeouts, total elapsed time, and basic latency statistics. It tests listener responsiveness only; it does not perform the Minecraft handshake, authentication, encryption, or play-state traffic.

## Flame Tracing

Run the example server with flame output:

```bash
VOID_METRICS_MODE=flame VOID_FLAME_OUTPUT=logs/void-flame.folded cargo run --release -p voidmc-example
```

Use the folded output with a flamegraph tool locally, then summarize hotspots and optimization decisions in [Optimization Log](./optimization-log.md). Keep `logs/void-flame.folded` out of git.

## Memory Sampling

Use `/usr/bin/time` for coarse memory evidence:

```bash
/usr/bin/time -v cargo run --release -p voidmc-example
```

Record at least the command, duration, player/client scenario, maximum resident set size, and commit hash in the result table.
