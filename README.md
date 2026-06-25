# Void

Void is a modular Minecraft server framework written in Rust.

Instead of shipping a monolithic server with every feature enabled, Void follows a minimal core plus composable plugins model. The project is an active Epitech Innovative Project, and APIs may evolve while the architecture is being stabilized.

## Objectives

- Provide a Minecraft-compatible server foundation built from scratch in Rust.
- Keep the runtime modular: networking, protocol, codec, game systems, commands, and data live in separate crates.
- Make gameplay features extensible through Bevy ECS systems, resources, observers, plugins, and commands.
- Keep protocol handling type-safe and testable through custom codec traits and derive macros.
- Document the architecture well enough for maintainers, reviewers, and future contributors to understand the system.

## Workspace

| Path | Purpose |
|---|---|
| `void/` | Core server framework: ECS app, systems, plugins, commands, client state, world state, metrics. |
| `void-example/` | Runnable example server and logging/metrics setup reference. |
| `void-net/` | Tokio-based TCP networking layer. |
| `void-protocol/` | Minecraft packet definitions and protocol types. |
| `void-codec/` | Binary `Encode`/`Decode` primitives. |
| `void-codec-macros/` | Procedural macros for codec derives. |
| `void-data/` | Generated Minecraft registry/block/entity data. |
| `void-docs/` | Rspress documentation site. |
| `pocs/tech-watch/` | Small technology-watch POCs used as decision evidence. |

## Prerequisites

- Rust stable toolchain
- Cargo
- Node.js and npm for the documentation site
- A Minecraft client compatible with the protocol version currently targeted by the server

## Quick Start

Build all crates:

```bash
cargo build --workspace
```

Run the example server:

```bash
cargo run -p voidmc-example
```

The example binds to `127.0.0.1:25565` by default. It creates runtime logs in `logs/`.

Run tests:

```bash
cargo test --workspace --all-features
```

Run formatting and lint checks:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Build the docs site:

```bash
cd void-docs
npm install
npm run build
```

## Release Run

Build and run the example server in release mode:

```bash
cargo build --release -p voidmc-example
./target/release/voidmc-example
```

Operational details, production caveats, logs, metrics, and deployment assumptions are documented in [Deployment](void-docs/docs/architecture/operational-readiness/deployment.md).

## Configuration

Void servers are configured through `ServerConfigBuilder`:

```rust
use voidmc::{ServerConfigBuilder, VoidServer};

fn main() {
    let config = ServerConfigBuilder::new()
        .address("127.0.0.1:25565")
        .tick_rate(20)
        .max_players(100)
        .view_distance(10)
        .motd("Welcome to Void")
        .build();

    VoidServer::new(config)
        .add_plugin(|app| {
            // Register systems, observers, resources, etc.
        })
        .run();
}
```

The example server also supports environment variables for diagnostics:

| Variable | Purpose |
|---|---|
| `RUST_LOG` | Controls tracing filters, for example `info` or `voidmc::network=debug`. |
| `VOID_METRICS_DEBUG` | Enables TPS metrics output. |
| `VOID_TPS_OUTPUT` | Sets the TPS CSV output path. |
| `VOID_METRICS_MODE=flame` | Enables flame trace output. |
| `VOID_FLAME_OUTPUT` | Sets the flame trace output path. |
| `VOID_PACKET_DEBUG` | Adds packet-level network debug logs. |

See [Server Configuration](void-docs/docs/reference/server/configuration.md) and [Configuration Examples](void-docs/docs/architecture/operational-readiness/configuration-example.md).

## Architecture

Void uses a dual-threaded runtime:

- Tokio network runtime for TCP connections and packet I/O.
- Bevy ECS game loop for world state, systems, events, plugins, commands, and player state.
- flume channels for all cross-thread communication.

Start with:

- [Architecture Overview](void-docs/docs/architecture/index.md)
- [Architecture Reference](void-docs/docs/reference/server/architecture.md)
- [ECS Reference](void-docs/docs/reference/server/ecs.md)
- [Protocol Codec](void-docs/docs/reference/protocol/codec.md)
- [Quality Standards](void-docs/docs/architecture/quality-standards.md)
- [Security and Reliability](void-docs/docs/architecture/operational-readiness/security-reliability.md)

## Quality Gates

The GitHub Actions CI runs formatting, Clippy with warnings denied, and workspace tests. Locally, use:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Rust's compiler checks, Clippy, rustfmt, and Cargo tests are the project's primary quality-analysis toolchain.

## Current Limitations

Void is still a student project and framework prototype. It is not yet a production-hardened public Minecraft server. Authentication/encryption hardening, abuse protection, persistence guarantees, and deployment automation are documented as current limitations in [Security and Reliability](void-docs/docs/architecture/operational-readiness/security-reliability.md).

## Contributing

Please read `CONTRIBUTING.md` before opening a pull request.

## Code of Conduct

Community expectations are documented in `CODE_OF_CONDUCT.md`.

## License

This project is licensed under the MIT License. See `LICENSE.md`.
