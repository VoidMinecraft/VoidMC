# VoidMC Technology Watch POCs

These mini POCs are intentionally separate from the production workspace. They are not part of the server implementation; they exist to make technology choices reproducible during the EIP defense.

## POCs

| POC | Command | Purpose |
|---|---|---|
| Async runtime comparison | `cargo run --manifest-path pocs/tech-watch/Cargo.toml --bin async_runtime_comparison --release` | Compare the shape and basic task-scheduling behavior of Tokio, async-std, and smol. |
| ECS modularity comparison | `cargo run --manifest-path pocs/tech-watch/Cargo.toml --bin ecs_modularity_comparison --release` | Compare Bevy ECS feature composition with a hand-written update loop. |

## Interpretation

These POCs are small decision-support artifacts. They do not replace production benchmarks. Their purpose is to demonstrate that the team tested alternatives concretely before selecting technologies for the main architecture.

