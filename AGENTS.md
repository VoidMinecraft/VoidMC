# Agent Guide

This file gives AI coding agents the minimum project context needed to make safe, productive changes in this repository.

## Runbook

- Build all crates: `cargo build --workspace`
- Run all tests: `cargo test --workspace`
- Format: `cargo fmt --all`
- Lint (warnings are errors): `cargo clippy --all-targets --all-features -- -D warnings`
- Run example server: `cargo run -p void-example`
- Docs site (from `void-docs/`): `npm install`, `npm run dev`, `npm run build`

## Workspace Map

- `void/`: core server framework (Bevy ECS app, systems, plugins, commands, world)
- `void-example/`: runnable example server and logging setup reference
- `void-net/`: async networking layer
- `void-protocol/`: protocol packet types and related tests
- `void-codec/`: encode/decode primitives and tests
- `void-codec-macros/`: proc-macro derives for codec traits

## Architecture Notes

- Runtime model is dual-threaded: Tokio network thread + Bevy ECS game thread.
- Cross-thread communication is via flume channels.
- Core gameplay/network behavior is implemented through plugins in `void/src/plugins/`.
- Default plugin registration lives in `void/src/plugins.rs` (`DefaultPlugins`).

## Change Patterns

- New plugin flow:
  1. Add plugin module in `void/src/plugins/`.
  2. Export/register it in `void/src/plugins.rs`.
  3. Add or update tests when behavior changes.
- Protocol changes usually touch both `void-protocol/` packet definitions and `void-codec/` encode/decode behavior.
- Keep changes focused; do not mix unrelated refactors.

## Conventions

- Use `tracing` for logs.
- Add tests for behavior changes.
- Update docs/examples when public APIs change.
- Check crate edition before using language features:
  - `void`, `void-example`, `void-net`, `void-protocol`: edition 2024
  - `void-codec`, `void-codec-macros`: edition 2021

## Reference Docs

- Project overview: [README.md](README.md)
- Contribution rules: [CONTRIBUTING.md](CONTRIBUTING.md)
- Architecture overview: [void-docs/docs/architecture/index.md](void-docs/docs/architecture/index.md)
- Server architecture details: [void-docs/docs/reference/server/architecture.md](void-docs/docs/reference/server/architecture.md)
- ECS reference: [void-docs/docs/reference/server/ecs.md](void-docs/docs/reference/server/ecs.md)
- Server configuration reference: [void-docs/docs/reference/server/configuration.md](void-docs/docs/reference/server/configuration.md)
- Protocol codec reference: [void-docs/docs/reference/protocol/codec.md](void-docs/docs/reference/protocol/codec.md)
- Gameplay commands reference: [void-docs/docs/reference/gameplay/commands.md](void-docs/docs/reference/gameplay/commands.md)

## Pitfalls

- `logs/` is generated output; do not commit log files.
- Run format, clippy, and tests before finalizing changes.
