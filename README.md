# Void

Void is a modular Minecraft server framework written in Rust.

Instead of shipping a monolithic server with every feature enabled, Void follows a minimal core + composable plugins model.

## Why Void

- Minimal core by default
- Gameplay features added through plugins
- Strong performance focus
- Ergonomic API for server developers

## Current Status

Active student project (Epitech Innovative Project).
The project is under active development and APIs may evolve.

## Workspace Overview

- `void/`: core server framework
- `void-example/`: example server binary
- `void-net/`: networking layer
- `void-protocol/`: protocol types/packets
- `void-codec/`: encoding/decoding primitives
- `void-codec-macros/`: proc macros for codec support

## Quick Start

Prerequisites:

- Rust (stable toolchain)
- Cargo

Build all crates:

```bash
cargo build --workspace
```

Run the example server:

```bash
cargo run -p void-example
```

Run tests:

```bash
cargo test --workspace
```

## Example

```rust
use void::{ServerBuilder, VoidServer};

fn main() {
    VoidServer::new(ServerBuilder::new().build())
        .add_plugin(|app| {
            // Register systems, observers, resources, etc.
        })
        .run();
}
```

## Contributing

Please read `CONTRIBUTING.md` before opening a Pull Request.

## Code of Conduct

Community expectations are documented in `CODE_OF_CONDUCT.md`.

## License

This project is licensed under the MIT License.
See `LICENSE.md`.
