# Integration Impact

This page maps evaluated technologies to their real integration points in VoidMC and explains the project impact.

| Technology | Where it is used | Why it was selected | Impact |
|---|---|---|---|
| Rust | Entire workspace | Memory safety, performance, strong type system | Enables low-level protocol code without garbage collection pauses. |
| Tokio | `void-net`, network thread in `void` | Mature async TCP runtime | Handles concurrent clients without blocking the game tick. |
| Bevy ECS | `void` game loop, components, resources, systems | Modular state model | Makes gameplay features composable through systems and plugins. |
| flume | Network/game communication channels | Clear producer-consumer ownership | Separates Tokio tasks from Bevy world mutation without shared locks. |
| `void-codec` | Protocol primitives and packet encoding | Minecraft-specific wire control | Supports VarInt, tagged packet enums, fixed-length fields, and remaining payloads. |
| `void-codec-macros` | Packet struct derives | Reduce boilerplate while keeping explicit wire attributes | Makes packet additions faster and less error-prone. |
| tracing | Server and example logging | Structured diagnostics | Produces actionable logs with targets, levels, and fields. |
| tracing-flame | Example server profiling mode | Flamegraph-compatible traces | Helps investigate slow paths without changing core server logic. |
| AGENTS.md | Repository root | Contextualize AI coding agents | Gives AI tools architecture rules, runbook commands, and change patterns. |

## Architecture Changes Caused by Technology Choices

- The runtime is explicitly split into a Tokio network domain and a Bevy ECS game domain.
- Cross-thread communication is intentionally limited to flume channels.
- Protocol definitions are strongly typed instead of treated as unstructured byte buffers.
- Plugins and commands are exposed as extension points so features can remain modular.
- Logging and metrics are built around structured traces rather than ad hoc terminal output.
- AI-assisted development is treated as a governed workflow through repository instructions.

## Measurable or Verifiable Impact

| Impact | Evidence |
|---|---|
| Protocol correctness | Unit and integration tests in `void-codec` and `void-protocol` |
| Modularity | Core plugins registered through `DefaultPlugins`; user extension via `VoidServer::add_plugin` |
| Runtime separation | Architecture docs and flume channel resources |
| Technology watch | Protocol diff, decision log, and benchmark artifacts |
| Maintainability | CI, Clippy, rustfmt, tests, and documentation site |

