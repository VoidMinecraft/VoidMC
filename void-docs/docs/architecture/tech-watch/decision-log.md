# Technology Decision Log

This log records technology choices made after evaluation. Dates reflect the project phase in which the decision became part of the architecture or documentation.

| Date | Decision | Alternatives | Outcome |
|---|---|---|---|
| 2025-Q4 | Use Rust as the implementation language | Java/Kotlin server extensions, C++, Go | Accepted for memory safety, performance, strong typing, and crate ecosystem fit. |
| 2025-Q4 | Use Tokio for networking | async-std, smol, blocking threads | Accepted for mature TCP support, broad documentation, and fit with many concurrent client tasks. |
| 2025-Q4 | Use Bevy ECS for game state | Custom ECS, hecs, ad hoc structs | Accepted because server state maps naturally to entities/components/resources and systems. |
| 2025-Q4 | Use flume channels between network and game threads | Arc/Mutex queues, RwLock shared state, crossbeam-channel | Accepted to keep ownership clear and avoid shared mutable state between runtime domains. |
| 2026-Q1 | Build `void-codec` instead of relying on Serde/bincode | Serde, bincode, manual packet functions only | Accepted because Minecraft uses protocol-specific VarInt, tagged packet IDs, fixed-length arrays, and remaining-byte fields. |
| 2026-Q2 | Add `void-codec-macros` derives | Hand-written Encode/Decode impls | Accepted to reduce boilerplate while keeping protocol-specific wire control. |
| 2026-Q2 | Track protocol changes from PaperMC sources | wiki-only tracking, manual trial-and-error | Accepted because Paper exposes vanilla packet registration order and current protocol field layouts. |
| 2026-Q2 | Add AGENTS.md for AI-assisted development | Generic AI prompts, no AI workflow | Accepted as an emerging workflow tool to reduce architecture drift when using coding agents. |
| 2026-Q2 | Add tracing and tracing-flame instrumentation | println logs, no profiling traces | Accepted for structured diagnostics and flamegraph-compatible traces during performance work. |

## Rejected or Deferred Choices

| Technology | Reason |
|---|---|
| Serde as the primary packet codec | Too generic for Minecraft-specific wire concerns such as VarInt fields, packet tags, and remaining-byte payloads. |
| Lock-shared game/network state | Makes ownership and tick consistency harder to reason about than channel handoff. |
| Pure hand-written packet codecs | Precise but too repetitive; derive macros preserve precision with less boilerplate. |
| Implementing every new protocol packet immediately | Deferred unless required by the current compatibility scope; protocol-watch notes separate mandatory from optional packets. |

## Supporting Artifacts

- [Technology Evaluations](/architecture/technology-evaluations)
- [Experiments](./experiments)
- POC package: `pocs/tech-watch`
- Protocol diff: `docs/protocol-diff-1.21.4-to-26.1.2.md`
