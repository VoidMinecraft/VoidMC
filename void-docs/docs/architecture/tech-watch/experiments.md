# Experiments and Benchmarks

VoidMC uses small, reproducible experiments before adopting infrastructure that affects protocol correctness or runtime behavior.

## POC 1: Async Runtime Comparison

| Field | Value |
|---|---|
| Goal | Compare Tokio, async-std, and smol on the kind of many-small-async-task shape used by client networking. |
| Artifact | `pocs/tech-watch/src/bin/async_runtime_comparison.rs` |
| Command | `cargo run --manifest-path pocs/tech-watch/Cargo.toml --bin async_runtime_comparison --release` |
| Decision supported | Use Tokio for production networking despite viable alternatives. |

This POC spawns simulated clients that process small asynchronous packet steps. It is intentionally small so the team can run it during reviews and discuss the tradeoff between raw behavior, API shape, ecosystem maturity, and production risk.

### Result Summary

Measured on 2026-06-24.

| Runtime | Result | Interpretation |
|---|---:|---|
| Tokio | 18.039 ms | Slower in this tiny timer-heavy POC, but strongest ecosystem fit for VoidMC. |
| async-std | 1.320 ms | Viable alternative with a comfortable API, but less aligned with the rest of our stack. |
| smol | 1.326 ms | Lightweight and fast in this POC, but smaller ecosystem and fewer production examples for our use case. |

**Decision:** keep Tokio. The POC shows that alternatives are technically plausible, but production selection also depends on TCP tooling, documentation, ecosystem support, and integration risk.

## POC 2: ECS Modularity Comparison

| Field | Value |
|---|---|
| Goal | Compare Bevy ECS feature composition with a hand-written update loop. |
| Artifact | `pocs/tech-watch/src/bin/ecs_modularity_comparison.rs` |
| Command | `cargo run --manifest-path pocs/tech-watch/Cargo.toml --bin ecs_modularity_comparison --release` |
| Decision supported | Use Bevy ECS as the game-state model. |

This POC runs the same small simulation twice: once with Bevy ECS systems/resources/components, and once with a manual vector update loop. Both produce the same final state, which is the point: the selected technology is justified by modularity and long-term maintainability, not by needing ECS for a toy update.

### Result Summary

Measured on 2026-06-24.

| Approach | Final position sum | Interpretation |
|---|---:|---|
| Bevy ECS | 529500.00 | Systems/resources compose naturally and mirror VoidMC's plugin model. |
| Manual loop | 529500.00 | Works for a prototype, but gameplay features become coupled as behavior grows. |

**Decision:** keep Bevy ECS. The POC demonstrates equivalent behavior while highlighting why the ECS model scales better for modular server features.

## Benchmark 1: Protocol Codec Shape

| Field | Value |
|---|---|
| Goal | Validate that a Minecraft-specific codec remains justified against a generic binary prototype. |
| Artifact | `void-codec/benches/codec_comparison.rs` |
| Compared approaches | `void-codec` protocol-shaped packet vs generic fixed-width binary prototype |
| Command | `cargo bench -p voidmc-codec --bench codec_comparison` |
| Decision supported | Use `void-codec` and `void-codec-macros` as the protocol layer. |

The generic prototype is intentionally simple: it encodes integers and lengths with fixed-width big-endian fields. This makes it easy to implement but does not match Minecraft's VarInt-heavy protocol. The benchmark measures both speed and the maintenance tradeoff: even when the generic version is competitive, it does not encode the right wire format.

### Result Summary

Measured on 2026-06-24 with `cargo bench -p voidmc-codec --bench codec_comparison`.

| Benchmark | Result |
|---|---:|
| `void_codec_protocol_shape` encode | 259.10 ns |
| `generic_fixed_width_prototype` encode | 25.149 ns |
| `void_codec_protocol_shape` decode | 296.07 ns |
| `generic_fixed_width_prototype` decode | 41.336 ns |

The generic prototype is faster because it is a deliberately simpler binary format, but it does not encode the Minecraft protocol shape. The accepted decision is still `void-codec` because correctness, VarInt support, tagged packet enums, and maintainability matter more than comparing against an incompatible wire format.

| Criterion | Conclusion |
|---|---|
| Raw speed | Generic fixed-width prototype wins |
| Wire-shape correctness | `void-codec` wins |
| Maintainability for packet definitions | `void-codec` wins through derive macros |
| Decision | Use `void-codec`; reject generic fixed-width encoding as the primary protocol layer |

## Benchmark 2: Cross-Thread Packet Handoff

| Field | Value |
|---|---|
| Goal | Validate the network-thread to game-thread handoff model. |
| Artifact | `void/benches/channel_handoff.rs` |
| Compared approaches | `flume::unbounded` channel vs `Arc<Mutex<VecDeque<_>>>` prototype |
| Command | `cargo bench -p voidmc --bench channel_handoff` |
| Decision supported | Keep flume as the cross-thread communication mechanism. |

The lock-based prototype represents the rejected architecture where network tasks and the game loop share mutable queues. It is useful as a comparison because it exposes the coordination cost and the weaker ownership story of shared mutable state.

### Result Summary

Measured on 2026-06-24 with `cargo bench -p voidmc --bench channel_handoff`.

| Benchmark | Result | Throughput |
|---|---:|---:|
| `flume_unbounded` | 79.645 us per 1,024 messages | 12.857 million messages/s |
| `mutex_vecdeque_prototype` | 57.881 us per 1,024 messages | 17.691 million messages/s |

The lock-based prototype is faster in this isolated micro-benchmark. The accepted decision is still flume because the production architecture values explicit ownership, clean thread boundaries, and lower risk of accidental shared-state mutation between Tokio tasks and Bevy ECS systems.

| Criterion | Conclusion |
|---|---|
| Raw micro-benchmark throughput | Mutex prototype wins |
| Ownership clarity | flume wins |
| Runtime boundary safety | flume wins |
| Production fit | flume wins |
| Decision | Use flume channels for packet, disconnect, and kick routing |

## Existing Applied Experiment: Protocol Migration

| Field | Value |
|---|---|
| Goal | Evaluate the impact of moving from Minecraft protocol 1.21.4 to 26.1.2. |
| Artifact | `docs/protocol-diff-1.21.4-to-26.1.2.md` |
| Compared approaches | Upgrade only required packets vs implement all newly introduced packets |
| Decision supported | Update required IDs/fields first, defer optional packets until features require them. |

This artifact demonstrates standards monitoring: the project reads upstream protocol sources, identifies compatibility changes, and turns the findings into implementation work.

## Recording Future Results

After running a benchmark, copy the important Criterion summary into this page or a dated page under `void-docs/docs/performance/`. Keep the command, machine context, result table, and conclusion together so the result is defensible during review.
