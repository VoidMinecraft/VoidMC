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
| Tick interval | About 50 ms between ticks at 20 TPS | `last_tick_ms` in the TPS CSV and the `TPS sample` log | Shows the game loop keeps its pace; includes the wait between ticks |
| Tick work time | Below the tick period; a warning above the configured `slow_tick_ms` | `tick_work_ms` in the slow-tick warning (with `metrics_debug`) and `voidmc_tick_duration_seconds` on `/metrics` (with observability), both from the start of `First` to the end of `Last` | Detects overloaded ECS systems |
| Codec latency | Sub-microsecond for representative small packets; low microseconds for chunk packet payloads | Criterion benchmarks | Packet throughput on the network thread |
| Channel handoff throughput | Millions of messages per second in micro-benchmarks | Criterion benchmark in `void/benches/channel_handoff.rs` | Validates network-to-ECS communication cost |
| Chunk packet serialization | Low microseconds for representative superflat chunk packets | Criterion benchmark in `void-protocol/benches/packet_chunk.rs` | Covers one of the heaviest clientbound packet paths |
| TCP connection acceptance | 100% success in local stress POC at documented client counts | `pocs/performance` TCP connect tool | Validates listener responsiveness under bursts |
| Memory footprint | Track with `/usr/bin/time -v` during release runs | Manual profiling command | Prevents hidden growth as systems are added |

## Runtime Controls

The example server exposes performance-oriented environment variables:

| Variable | Purpose |
| --- | --- |
| `VOID_METRICS_DEBUG=1` | Enables TPS sampling and the TPS CSV in `void-example` (`true` and `yes` also work). Without it nothing is sampled and no CSV is written |
| `VOID_TPS_OUTPUT=logs/tps-demo.csv` | Writes TPS samples to a chosen CSV file; needs `VOID_METRICS_DEBUG=1`. Default: `logs/tps-<unix-ms>.csv` |
| `VOID_METRICS_MODE=flame` | Writes a folded flame trace (setting `VOID_FLAME_OUTPUT` does too). Independent of TPS sampling |
| `VOID_FLAME_OUTPUT=logs/void-flame.folded` | Selects the folded flame-trace output path. Default: `logs/trace-<unix-ms>.folded` |
| `VOID_FLAME_INCLUDE_IDLE=1` | Includes idle time so the trace shows overall utilization |
| `VOID_PACKET_DEBUG=1` | Enables verbose packet logging for protocol debugging |
| `VOID_OBSERVABILITY_ADDR=9464` | Serves `/health` and Prometheus `/metrics` on `127.0.0.1:9464` ([reference](../reference/server/observability.md)) |

Relative paths are resolved against the working directory. Missing parent
directories are created and an existing file is replaced. When a file cannot
be created (the path is a directory, lies under a regular file, or is not
writable), the server logs a warning that names the path and the reason, and
runs without that output. Otherwise it logs `Writing TPS samples to <path>` or
`Writing flame trace to <path>`. See [Benchmarking](./benchmarking.md) for the
file formats.

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
