# Developer Guides

Welcome to the Void developer guides. Here you'll find comprehensive documentation for getting started, developing, and testing Void.

## Quick Navigation

### 🚀 [Getting Started](/guide/getting-started)

New to Void? Start here! Learn how to:

- Install and run the server
- Understand the project structure
- Connect with a Minecraft client
- Learn key architectural concepts

### 🔧 [Development Guide](/guide/development)

Set up your development environment and learn best practices:

- IDE setup (VS Code, CLion)
- Code organization and naming conventions
- Working with the protocol and macros
- Async/Tokio patterns
- Git workflow
- Debugging and optimization

### ✅ [Testing Guide](/guide/testing)

Write and run tests with confidence:

- Unit tests, integration tests, documentation tests
- Testing strategies for each component
- Running the test suite
- Coverage goals and best practices
- Common test patterns

## Topics by Role

### For Backend Developers

1. **Getting Started** - Set up and understand the basics
2. **Development Guide** - Configure your environment
3. **Architecture** (from main nav) - Understand the design
4. **Testing Guide** - Write quality code

**Typical workflow:**

```bash
# Setup
cargo build && cargo test

# Development
git checkout -b feature/your-feature
# ... make changes ...

# Test and verify
cargo fmt && cargo clippy && cargo test

# Submit PR
git push origin feature/your-feature
```

### For Performance Optimizers

1. **Getting Started** - Understand the project
2. **Performance Metrics** (from main nav) - Learn what we measure
3. **Benchmarking** (from main nav) - Profile and optimize
4. **Development Guide** - Use profiling tools

**Key tools:**

- `cargo bench` - Run benchmarks
- `cargo flamegraph` - Visualize hot spots
- Performance metrics dashboard

### For QA & Testing

1. **Testing Guide** - Understand our test strategy
2. **Performance Metrics** - Learn performance testing
3. **Getting Started** - Set up test environment

**Key responsibilities:**

- Running test suite
- Manual testing scenarios
- Performance regression detection
- Documentation of test results

## Common Tasks

### Running the Server

```bash
# Development (debug build)
cargo run -p voidmc-example

# Release (optimized)
cargo run --release -p voidmc-example

# With custom log filters
RUST_LOG=debug cargo run --release -p voidmc-example
```

Server starts at `127.0.0.1:25565`

### Testing Changes

```bash
# All tests
cargo test --workspace --all-features

# Specific test
cargo test test_name

# With output
cargo test -- --nocapture

# Single-threaded
cargo test -- --test-threads=1
```

### Code Quality

```bash
# Format
cargo fmt --all

# Lint (the CI command)
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Documentation
cargo doc --no-deps --open
```

### Performance Analysis

```bash
# Benchmarks
cargo bench --workspace --all-features

# Flame graph of the example server (needs `cargo install inferno`)
make flame
open logs/void-flame-active.svg

# Code coverage (the CI tool)
cargo llvm-cov --workspace --all-features
```

## Project Structure

```
void/                 # voidmc: core framework (ECS app, plugins, systems, commands, world)
  src/
    app.rs            # VoidServer entry point
    server.rs         # Tokio accept loop
    client.rs         # Per-connection task
    network.rs        # Packet ingest and dispatch
    plugins/          # Connection lifecycle and gameplay plugins
    systems/          # Tick systems (keep-alive, chunks, physics, ...)
    world/            # Chunks, generation, block entities

void-protocol/        # voidmc-protocol: packet definitions
  src/
    clientbound/      # Server → Client (status/, login/, configuration/, play/)
    serverbound/      # Client → Server

void-codec/           # voidmc-codec: Encode/Decode traits and primitives
void-codec-macros/    # voidmc-codec-macros: derive macros
void-net/             # voidmc-net: TCP socket and framing
void-data/            # voidmc-data: vanilla registry data baked at build time
void-world-io/        # voidmc-world-io: chunk persistence plugin
void-example/         # voidmc-example: runnable example server
void-demo/            # voidmc-demo: Alpine Rush racing demo
```

## Key Concepts

### Protocol States

```
Handshake → Status → Login → Configuration → Play
```

Each state handles different packets.

### Async Architecture

- Non-blocking I/O with Tokio
- One task per client
- Shared state via `Arc<Mutex<T>>`

### Type Safety

- Rust's type system prevents many errors
- Macros auto-derive Encode/Decode
- Protocol validated at compile-time

## Getting Help

- **Questions?** Open a GitHub Discussion
- **Found a bug?** Report an issue
- **Need help?** Check existing discussions
- **Want to contribute?** Read the Contributing guide

## Next Steps

Choose your path:

1. **I want to build features** → [Development Guide](/guide/development)
2. **I want to optimize performance** → [Performance Metrics](/performance)
3. **I want to ensure quality** → [Testing Guide](/guide/testing)
4. **I want to contribute** → [Contributing Guide](/contributing)
5. **I want to understand the design** → [Architecture](/architecture)

---

**Happy coding!** 🎮 If you have questions, don't hesitate to reach out!
