# Watch Sources

This page records the sources used to follow ecosystem changes, evaluate options, and detect protocol updates that matter to VoidMC.

## Core Rust and Tooling

| Source | Usage |
|---|---|
| [Rust Blog](https://blog.rust-lang.org/) | Language releases, compiler behavior, ecosystem announcements |
| [Rust Reference](https://doc.rust-lang.org/reference/) | Language semantics when implementing macros and low-level encoding |
| [Cargo Book](https://doc.rust-lang.org/cargo/) | Workspace, benchmarking, release, and dependency guidance |
| [Clippy Documentation](https://doc.rust-lang.org/clippy/) | Lint policy and quality automation |

## Async, ECS, and Runtime Architecture

| Source | Usage |
|---|---|
| [Tokio Documentation](https://docs.rs/tokio) | Async runtime APIs, task model, TCP networking |
| [Tokio Blog](https://tokio.rs/blog) | Runtime guidance and performance recommendations |
| [Bevy ECS Documentation](https://docs.rs/bevy_ecs) | ECS schedule, components, resources, observers |
| [flume Documentation](https://docs.rs/flume) | Channel behavior and cross-thread communication |
| [tracing Documentation](https://docs.rs/tracing) | Structured logging and instrumentation |
| [tracing-flame Documentation](https://docs.rs/tracing-flame) | Flamegraph-compatible runtime traces |

## Minecraft Protocol and Data

| Source | Usage |
|---|---|
| [wiki.vg Minecraft Protocol](https://wiki.vg/Protocol) | Protocol concepts and packet structure cross-checking |
| [PaperMC GitHub](https://github.com/PaperMC/Paper) | Vanilla-derived source of truth for packet IDs and field layouts |
| [Minecraft generated reports](https://minecraft.wiki/w/Minecraft_Wiki:Projects/wiki.vg_merge/Protocol) | Registry and data-pack reference material |
| Local protocol diff: `docs/protocol-diff-1.21.4-to-26.1.2.md` | Project-specific watch artifact for version migration |

## Comparative Technology Sources

| Source | Usage |
|---|---|
| [Serde Documentation](https://serde.rs/) | Baseline comparison for serialization ergonomics |
| [bincode Documentation](https://docs.rs/bincode) | Binary serialization comparison point |
| [async-std Documentation](https://docs.rs/async-std) | Alternative async runtime comparison |
| [smol Documentation](https://docs.rs/smol) | Lightweight async runtime comparison |
| [hecs Documentation](https://docs.rs/hecs) | Alternative ECS comparison |

## Community and Team Learning Channels

| Channel | Usage |
|---|---|
| GitHub issues and pull requests | Track implementation decisions, reviews, regressions, and protocol fixes |
| Rust community discussions | Resolve Rust-specific design questions and follow ecosystem direction |
| Minecraft protocol communities | Cross-check protocol interpretations and compatibility questions |
| Team review notes | Share findings before decisions are integrated into the codebase |

When a source leads to a decision, the result is recorded in the [Decision Log](./decision-log) instead of staying as an isolated bookmark.

