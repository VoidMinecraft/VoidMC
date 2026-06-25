# Optimization Log

This page records the performance choices already present in the architecture, including tradeoffs and current limitations.

## 2026-06-24: Channel Handoff Boundary

| Item | Notes |
| --- | --- |
| Before/prototype | Shared `Mutex<VecDeque<_>>` queue benchmark |
| Current choice | `flume` channels between Tokio networking and the Bevy ECS loop |
| Evidence | `cargo bench -p voidmc --bench channel_handoff` |
| Result | Mutex prototype median: 84.18 us per 1024 messages; `flume` median: 104.77 us per 1024 messages |
| Decision | Keep `flume` despite the slower synthetic micro-benchmark |

Rationale: the channel boundary matches the production architecture better than a shared queue. It keeps network producers and ECS consumers decoupled, avoids exposing lock ownership to gameplay systems, and is easier to reason about when plugins are added.

Limitation: this benchmark measures local handoff only. It does not include full packet decode, per-client routing, Bevy scheduling, or backpressure behavior under real player traffic.

## 2026-06-24: Packet Ingest Budget

| Item | Notes |
| --- | --- |
| Before/risk | Draining every pending network packet in one tick can starve gameplay systems during bursts |
| Current choice | `max_packets_per_tick` and `packet_ingest_budget_ms` in `ServerConfig` |
| Default | `max_packets_per_tick = 1000`, `packet_ingest_budget_ms = 4` |
| Evidence | `void/src/network.rs`, configuration reference, TPS CSV output |
| Result | Idle release sample stayed around 20 TPS with about 50 ms ticks |

Rationale: bounded packet ingest gives the game loop an explicit fairness control. It can trade immediate packet drain speed for stable tick cadence.

Limitation: the current docs include an idle TPS sample and connection burst sample. A future full protocol stress test should vary packet rates while recording TPS and queue depth.

## 2026-06-24: Chunk Generation Budget

| Item | Notes |
| --- | --- |
| Before/risk | Generating all requested chunks in a single tick can create slow ticks |
| Current choice | `max_chunk_generations_per_tick` in `ServerConfig` |
| Default | `8` chunks per tick |
| Evidence | `void/src/systems/chunk.rs`, configuration reference |
| Result | Chunk packet serialization now has a dedicated Criterion benchmark |

Rationale: chunk work is one of the heavier gameplay paths. A per-tick generation cap gives the server a simple control before more advanced streaming or priority queues are needed.

Limitation: current chunk benchmarks cover packet construction and encoding, not large world-generation workloads.

## 2026-06-24: Slow Tick Warning

| Item | Notes |
| --- | --- |
| Before/risk | Slow ticks could remain invisible until users noticed lag |
| Current choice | `slow_tick_ms` logs warnings from `MetricsPlugin` |
| Default | `200` ms |
| Evidence | `void/src/metrics.rs`, TPS CSV output, tracing logs |
| Result | Performance diagnosis has a runtime signal tied to tick duration |

Rationale: this turns performance problems into visible operational events. It is intentionally conservative because brief startup or debug spikes should not be treated as gameplay failure.

Limitation: slow tick warnings are logs, not automated alerts.

## 2026-06-24: Custom Codec and Derive Macros

| Item | Notes |
| --- | --- |
| Before/prototype | Generic fixed-width encode/decode benchmark |
| Current choice | `void-codec` plus derive macros for packet definitions |
| Evidence | `cargo bench -p voidmc-codec --bench codec_comparison` and `cargo bench -p voidmc-protocol --bench packet_chunk` |
| Result | Generic prototype is faster in a tiny micro-benchmark; `void-codec` representative decode median is 297.31 ns and chunk packet encode median is 2.44 us |
| Decision | Keep the protocol-aware codec |

Rationale: Minecraft packet correctness depends on tagged packet IDs, VarInt lengths, packet-specific layouts, and clear derive behavior. The generic fixed-width prototype is a useful speed reference, not a production-compatible codec.

Limitation: future work should add regression thresholds or archived baselines before large protocol rewrites.
