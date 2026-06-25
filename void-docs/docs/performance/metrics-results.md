# Metrics & Results

This page stores summarized performance results that can be shown without committing generated logs, Criterion HTML reports, flame traces, or CSV files.

## 2026-06-24 Local Baseline

Environment: local development machine, release-mode benchmarks, Criterion defaults. These numbers are comparative evidence, not a production capacity promise.

### TPS Sample

Command:

```bash
VOID_METRICS_DEBUG=1 VOID_TPS_OUTPUT=/tmp/voidmc-tps-demo.csv cargo run --release -p voidmc-example
```

Sample rows from a short idle release run:

| Sample | TPS | Window | Last tick | Total ticks |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 20.80 | 1009.82 ms | 50.10 ms | 21 |
| 2 | 19.97 | 1001.62 ms | 50.07 ms | 41 |
| 3 | 19.97 | 1001.74 ms | 50.07 ms | 61 |
| 4 | 19.96 | 1001.81 ms | 50.08 ms | 81 |
| 5 | 19.96 | 1001.90 ms | 50.09 ms | 101 |
| 6 | 19.97 | 1001.71 ms | 50.07 ms | 121 |
| 7 | 19.97 | 1001.74 ms | 50.09 ms | 141 |
| 8 | 19.96 | 1001.76 ms | 50.10 ms | 161 |
| 9 | 19.96 | 1001.99 ms | 50.21 ms | 181 |
| 10 | 19.96 | 1001.95 ms | 50.05 ms | 201 |

Conclusion: idle release-mode server stayed at the expected 20 TPS cadence in this short run.

### Codec Comparison

Command:

```bash
cargo bench -p voidmc-codec --bench codec_comparison
```

| Benchmark | Median estimate | Throughput estimate | Conclusion |
| --- | ---: | ---: | --- |
| `encode/void_codec_protocol_shape` | 328.43 ns | 3.04 million packets/s | Protocol-aware codec path is comfortably below the packet latency target |
| `encode/generic_fixed_width_prototype` | 40.61 ns | 24.62 million packets/s | Faster micro-benchmark but does not model VarInt/tagged packet shape |
| `decode/void_codec_protocol_shape` | 297.31 ns | 3.36 million packets/s | Decode path is below the packet latency target |
| `decode/generic_fixed_width_prototype` | 42.18 ns | 23.71 million packets/s | Faster only for the simplified fixed-width prototype |

Conclusion: the generic prototype wins this narrow micro-benchmark, but it is not a valid Minecraft protocol substitute. The selected `void-codec` path keeps explicit protocol layout, VarInt handling, and derive-based packet definitions.

### Channel Handoff

Command:

```bash
cargo bench -p voidmc --bench channel_handoff
```

| Benchmark | Median estimate | Throughput estimate | Conclusion |
| --- | ---: | ---: | --- |
| `flume_unbounded` | 104.77 us per 1024 messages | 9.77 million messages/s | Production choice; simple multi-producer handoff between Tokio and Bevy |
| `mutex_vecdeque_prototype` | 84.18 us per 1024 messages | 12.16 million messages/s | Faster in this single micro-benchmark, but couples producers and consumers through explicit locking |

Conclusion: the rejected lock prototype can be faster in this synthetic case. VoidMC keeps `flume` because it matches the architecture boundary, avoids shared mutable queues in gameplay systems, and makes ownership easier to reason about.

### Chunk & Representative Packet Benchmark

Command:

```bash
cargo bench -p voidmc-protocol --bench packet_chunk
```

| Benchmark | Median estimate | Payload size | Conclusion |
| --- | ---: | ---: | --- |
| `chunk_to_packet` | 4.90 us | Derived packet data | Converts a representative superflat chunk into a packet payload in low microseconds |
| `encode_chunk_data_and_light` | 2.58 us | 55,892 bytes | Chunk payload serialization is measurable and reproducible |
| `encode_manual_play_chunk` | 2.44 us | 55,893 bytes | Packet ID wrapper adds negligible cost |
| `encode_clientbound_keep_alive` | 37.51 ns | 9 bytes | Representative small clientbound packet is sub-microsecond |
| `encode_serverbound_position` | 94.06 ns | 26 bytes | Representative movement packet encode is sub-microsecond |
| `decode_serverbound_position` | 19.21 ns | 26 bytes | Representative movement packet decode is sub-microsecond |

Conclusion: chunk packets are the meaningful serialization hotspot today; small control and movement packets are not.

### TCP Connect Stress POC

Commands:

```bash
cargo run --release -p voidmc-example
cargo run --manifest-path pocs/performance/Cargo.toml --release --bin tcp_connect_stress -- --addr 127.0.0.1:25565 --clients 64 --timeout-ms 1000
```

| Metric | Value |
| --- | ---: |
| Connection attempts | 64 |
| Successful connections | 64 |
| Failed connections | 0 |
| Timed out connections | 0 |
| Total elapsed | 0.570 ms |
| Average latency | 0.272 ms |
| Minimum latency | 0.076 ms |
| Maximum latency | 0.502 ms |

Conclusion: the local TCP listener accepted a 64-client burst without failures. This POC validates TCP accept responsiveness only; full Minecraft handshake/login stress remains future work.

## Memory Tracking

Memory is currently tracked manually when needed:

```bash
/usr/bin/time -v cargo run --release -p voidmc-example
```

Future result tables should include maximum resident set size, player/client count, test duration, and enabled plugins.
