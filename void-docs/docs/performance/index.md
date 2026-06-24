# Performance Measurement

This section is the evidence trail for the school objective **"Mesurer, tester et optimiser les performances techniques"**. The project goal is to prove performance work with repeatable measurements: define KPIs, run benchmarks or stress tools, compare alternatives, document bottlenecks, and keep optimization decisions visible.

## Defense Path

Use this order during a review:

1. Start with the KPIs below and the latest dated tables in [Metrics & Results](./metrics-results.md).
2. Show the exact commands in [Benchmarking & Profiling](./benchmarking.md).
3. Explain the optimization tradeoffs in [Optimization Log](./optimization-log.md).
4. Close with the school mapping in [Requirement Checklist](./requirement-checklist.md).

## KPIs

| KPI | Target | Measurement source | Why it matters |
| --- | ---: | --- | --- |
| Tick rate | About 20 TPS | `MetricsPlugin` CSV output | Main game-loop health and player responsiveness |
| Tick time | About 50 ms per tick, warnings above configured `slow_tick_ms` | `last_tick_ms` in TPS CSV and tracing logs | Detects overloaded ECS systems |
| Codec latency | Sub-microsecond for representative small packets; low microseconds for chunk packet payloads | Criterion benchmarks | Packet throughput on the network thread |
| Channel handoff throughput | Millions of messages per second in micro-benchmarks | Criterion benchmark in `void/benches/channel_handoff.rs` | Validates network-to-ECS communication cost |
| Chunk packet serialization | Low microseconds for representative superflat chunk packets | Criterion benchmark in `void-protocol/benches/packet_chunk.rs` | Covers one of the heaviest clientbound packet paths |
| TCP connection acceptance | 100% success in local stress POC at documented client counts | `pocs/performance` TCP connect tool | Validates listener responsiveness under bursts |
| Memory footprint | Track with `/usr/bin/time -v` during release runs | Manual profiling command | Prevents hidden growth as systems are added |

## Runtime Controls

The example server exposes performance-oriented environment variables:

| Variable | Purpose |
| --- | --- |
| `VOID_METRICS_DEBUG=1` | Enables TPS metrics collection in `void-example` |
| `VOID_TPS_OUTPUT=logs/tps-demo.csv` | Writes TPS samples to a chosen CSV file |
| `VOID_METRICS_MODE=flame` | Enables flame tracing mode in the example logging setup |
| `VOID_FLAME_OUTPUT=logs/void-flame.folded` | Selects the folded flame-trace output path |
| `VOID_PACKET_DEBUG=1` | Enables verbose packet logging for protocol debugging |

Generated `logs/`, raw Criterion output, flame traces, and `target/` artifacts stay out of git. Commit summarized Markdown tables instead.

## Primary Commands

```bash
cargo bench -p voidmc-codec --bench codec_comparison
cargo bench -p voidmc --bench channel_handoff
cargo bench -p voidmc-protocol --bench packet_chunk
```

```bash
VOID_METRICS_DEBUG=1 VOID_TPS_OUTPUT=logs/tps-demo.csv cargo run --release -p voidmc-example
```

```bash
cargo run --manifest-path pocs/performance/Cargo.toml --release --bin tcp_connect_stress -- --addr 127.0.0.1:25565 --clients 64 --timeout-ms 1000
```
