# Technology Evaluations

VoidMC is a high-performance Minecraft server framework written from scratch in Rust. The project cannot rely on default web-service patterns: it has a binary game protocol, long-lived TCP clients, a fixed-rate game loop, and a strong modularity goal.

This page records the main technology comparisons behind the architecture. The broader watch process is documented in [Tech Watch](/architecture/tech-watch/).

## Evaluation Matrix

| Area | Selected choice | Main alternatives | Decision |
|---|---|---|---|
| Packet codec | Custom `void-codec` + derives | Serde, bincode, hand-written only | Accepted |
| Async runtime | Tokio | async-std, smol, blocking threads | Accepted |
| Game state model | Bevy ECS | Custom ECS, hecs, ad hoc structs | Accepted |
| Thread handoff | flume channels | Mutex/RwLock queues, shared state | Accepted |
| Observability | tracing + tracing-flame | println logs, external-only profilers | Accepted |
| AI workflow | AGENTS.md contextualization | Generic AI prompts | Accepted |

## Custom `void-codec` vs Serde/bincode

Minecraft's protocol is not a generic object serialization format. It uses packet IDs, VarInt and VarLong fields, state-dependent packet enums, fixed-length vectors, NBT payloads, and fields that consume the remaining bytes of a packet.

| Criterion | `void-codec` | Serde/bincode |
|---|---|---|
| Minecraft wire compatibility | Direct support for protocol-specific attributes | Requires adapters or custom serializers for many fields |
| Type safety | Packet structs keep domain types and explicit attributes | Possible, but VarInt/fixed-length intent is less visible |
| Boilerplate | Derive macros generate repetitive Encode/Decode impls | Ergonomic for generic data formats |
| Debuggability | Wire behavior is local to packet type and attributes | Behavior can be hidden in serializer configuration |
| Exit cost | Medium; packet derives are project-specific | Lower for generic formats, but format mismatch remains |

**Decision:** keep a custom codec. Serde remains useful for JSON/NBT-adjacent data, but it is not the primary network packet codec.

**Evidence:** `void-codec`, `void-codec-macros`, extensive codec tests, and the `codec_comparison` benchmark.

## Tokio vs async-std/smol

VoidMC needs to hold long-lived TCP connections, read and write framed packets, and keep the game loop isolated from network I/O.

| Criterion | Tokio | async-std | smol |
|---|---|---|---|
| Ecosystem maturity | Very high | Moderate | Focused and lightweight |
| TCP/runtime tooling | Broad and battle-tested | Good, smaller ecosystem | Good, smaller ecosystem |
| Documentation and examples | Extensive | Good | Smaller |
| Integration risk | Low | Medium | Medium |
| Fit for many clients | Strong | Plausible | Plausible |

**Decision:** use Tokio for the networking layer because it minimizes runtime risk and gives the team the most documentation and ecosystem support.

**Evidence:** `void-net` uses Tokio, `VoidServer` starts a dedicated Tokio runtime for networking, and `pocs/tech-watch/src/bin/async_runtime_comparison.rs` compares Tokio with async-std and smol.

## Bevy ECS vs Custom ECS/hecs

VoidMC wants ultra-modular gameplay. Player state, chunk state, command state, events, and future plugin behavior all benefit from a data-oriented model.

| Criterion | Bevy ECS | Custom ECS | hecs |
|---|---|---|---|
| Scheduling model | Built-in schedules and systems | Must be designed and maintained | Minimal |
| Resources/components | Mature API | Full control but high cost | Good core API |
| Observer/event fit | Strong with Bevy observers | Must be built | Must be built |
| Learning cost | Moderate | High for maintainers | Low to moderate |
| Project fit | Strong for plugin-oriented server logic | Risky long-term maintenance | Good but less complete scheduling story |

**Decision:** use Bevy ECS for the game thread. It gives the project an existing component/resource/system model and leaves the team free to focus on Minecraft behavior.

**Evidence:** `void` components, resources, systems, observers, `DefaultPlugins`, `VoidServer::add_plugin`, and `pocs/tech-watch/src/bin/ecs_modularity_comparison.rs`.

## flume Channels vs Mutex/RwLock Shared Queues

The architecture separates the Tokio network thread from the Bevy game thread. The key question is how packets cross that boundary.

| Criterion | flume channels | Mutex/RwLock queue |
|---|---|---|
| Ownership model | Explicit producer/consumer channels | Shared mutable state |
| Game thread safety | Packets are drained into ECS on schedule | Easier to accidentally mutate from the wrong side |
| Failure behavior | Send/receive errors are explicit | Lock poisoning and contention must be handled |
| Implementation complexity | Low | Medium |
| Debuggability | Channel direction is visible in architecture docs | Queue ownership can become implicit |

**Decision:** use flume channels for incoming packets, outgoing packets, disconnects, and kicks.

**Evidence:** architecture docs, `void/src/app.rs`, network resources, and the `channel_handoff` benchmark.

## tracing and tracing-flame vs Ad Hoc Logs

Server debugging needs more than terminal strings. The team needs structured events, file output, packet-level debug switches, and performance traces.

| Criterion | tracing + tracing-flame | println/ad hoc logs |
|---|---|---|
| Structured context | Native fields and targets | Manual formatting |
| Filtering | `RUST_LOG` and directives | Manual switches |
| File output | Supported through subscriber layers | Custom work |
| Profiling | Flamegraph-compatible trace layer | Not available |

**Decision:** use tracing for logs and tracing-flame for optional profiling in the example server.

**Evidence:** `void-example/src/main.rs`, metrics docs, and changelog entries.

## AI Agent Contextualization with AGENTS.md

AI coding tools are useful but risky in a custom architecture. Generic prompts often miss project-specific boundaries such as the dual-threaded runtime, flume-only cross-thread communication, and plugin registration flow.

| Criterion | AGENTS.md contextualization | Generic AI prompts |
|---|---|---|
| Architecture consistency | Repository rules are always available | Depends on prompt quality |
| Onboarding value | Helps humans and agents | Helps only a single interaction |
| Drift prevention | Documents runbook and pitfalls | Easy to forget constraints |
| Cost | Low maintenance file | Low initial cost, higher correction cost |

**Decision:** keep AGENTS.md as a first-class engineering artifact for AI-assisted development.

**Evidence:** `AGENTS.md`, project runbook, architecture notes, and change patterns.

## Resulting Architecture Principles

- Keep networking asynchronous and isolated from ECS mutation.
- Keep game state modular through components, resources, systems, plugins, and events.
- Keep protocol encoding explicit and tested.
- Prefer reproducible experiments before adopting foundational infrastructure.
- Document rejected choices so future contributors understand the tradeoffs.
