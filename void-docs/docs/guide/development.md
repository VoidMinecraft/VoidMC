# Development Guide

This guide covers the development setup, workflow, and best practices for working on Void.

## Development Environment Setup

### Prerequisites

```bash
# Rust (latest stable)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# Verify installation
rustc --version  # rustc 1.XX.X
cargo --version  # cargo 1.XX.X
```

### IDE Setup

#### VS Code (Recommended)

```bash
# Install extensions
code --install-extension rust-lang.rust-analyzer
code --install-extension serayuzgur.crates
code --install-extension vadimcn.vscode-lldb
```

Create `.vscode/settings.json`:

```json
{
  "rust-analyzer.checkOnSave.command": "clippy",
  "rust-analyzer.checkOnSave.extraArgs": ["--", "-D", "warnings"],
  "[rust]": {
    "editor.formatOnSave": true,
    "editor.defaultFormatter": "rust-lang.rust-analyzer"
  }
}
```

#### JetBrains CLion

- Install Rust plugin from marketplace
- Enable `cargo check` instead of `cargo build`
- Configure run/debug configurations

### Building the Project

```bash
# Debug build (fast compile, slow runtime)
cargo build

# Release build (slow compile, fast runtime)
cargo build --release

# Run directly
cargo run --release

# Run with arguments
cargo run --release -- --help

# Watch mode (requires cargo-watch)
cargo install cargo-watch
cargo watch -x build
```

## Code Organization

### Crate Structure

```
void/                      # Main server
  src/
    lib.rs                # Library exports
    main.rs               # Binary entry point
    server.rs             # Server implementation
    client.rs             # Client handler
    game.rs               # Game state

void-codec/                # Codec traits
  src/
    lib.rs
    primitives/           # Basic types (VarI32, UUID, etc.)
    error.rs              # Error types

void-protocol/             # Protocol definitions
  src/
    lib.rs
    clientbound.rs
    serverbound.rs
    clientbound/
      status/
      login/
      play/

void-codec-macros/         # Macro implementations
  src/
    lib.rs
    encode.rs
    decode.rs
```

### Naming Conventions

- **Modules**: `snake_case`
- **Types**: `PascalCase`
- **Functions**: `snake_case`
- **Constants**: `SCREAMING_SNAKE_CASE`
- **Lifetimes**: `'a`, `'b`, etc.
- **Type parameters**: `T`, `U` (or more descriptive like `P: Player`)

## Working with Macros

### Procedural Macros (void-codec-macros)

Located in `void-codec-macros/src/`:

```rust
// A derive macro for Encode
#[proc_macro_derive(Encode, attributes(codec))]
pub fn derive_encode(input: TokenStream) -> TokenStream {
    // Expand to Encode implementation
}
```

**Testing macros:**

```bash
cargo test -p void-codec-macros
```

**Debugging expansion:**

```bash
cargo expand
```

## Working with the Protocol

### Adding a New Packet Type

1. **Define struct in appropriate module**:

```rust
// void-protocol/src/clientbound/play/spawn_entity.rs
use voidmc_codec::{Encode, Decode};

#[derive(Encode, Decode, Debug)]
pub struct SpawnEntity {
    pub entity_id: u32,
    #[codec(varint32)]
    pub x: i32,
    pub y: i64,
    pub z: i64,
    pub yaw: u8,
    pub pitch: u8,
    pub data: i32,
}
```

2. **Add to packet enum**:

```rust
// void-protocol/src/clientbound/play/mod.rs
#[derive(Encode, Decode)]
#[codec(tagged)]
pub enum PlayPacket {
    #[codec(packet_id = 0x00)]
    BundleDelimiter(BundleDelimiter),
    // ... other packets ...
    #[codec(packet_id = 0x01)]
    SpawnEntity(SpawnEntity),
}
```

3. **Add to handler**:

```rust
// void/src/client.rs
async fn handle_play_packet(&mut self, packet: PlayPacket) -> Result<()> {
    match packet {
        PlayPacket::SpawnEntity(spawn) => {
            self.handle_spawn_entity(spawn).await?;
        }
        // ...
    }
    Ok(())
}
```

4. **Write tests**:

```rust
#[test]
fn test_spawn_entity_encode_decode() {
    let original = SpawnEntity {
        entity_id: 42,
        x: 100,
        y: 64,
        z: 200,
        yaw: 180,
        pitch: 0,
        data: 0,
    };

    let mut buf = Vec::new();
    original.encode(&mut buf);

    let mut reader = &buf[..];
    let decoded = SpawnEntity::decode(&mut reader).unwrap();

    assert_eq!(original, decoded);
}
```

## Working with Async Code

### Tokio Patterns

```rust
// Simple spawn task
tokio::spawn(async {
    // This runs in background
});

// Join multiple tasks
let handles: Vec<_> = (0..10)
    .map(|_| tokio::spawn(async { /* ... */ }))
    .collect();

futures::future::join_all(handles).await;

// Using Arc<Mutex> for shared state
use std::sync::Arc;
use tokio::sync::Mutex;

let state = Arc::new(Mutex::new(GameState::new()));

// Clone for each task
let state_clone = Arc::clone(&state);
tokio::spawn(async move {
    let mut state = state_clone.lock().await;
    state.do_something();
});
```

### Common Pitfalls

❌ **Don't:**

```rust
// Blocking in async code
async fn bad() {
    std::thread::sleep(Duration::from_secs(1));  // BLOCKS!
}
```

✅ **Do:**

```rust
// Non-blocking delay
async fn good() {
    tokio::time::sleep(Duration::from_secs(1)).await;
}
```

## Code Style

### Formatting

```bash
# Format code
cargo fmt

# Check formatting
cargo fmt --check
```

### Linting

```bash
# Run clippy
cargo clippy

# Fix some issues automatically
cargo clippy --fix

# Strict mode (fail on warnings)
cargo clippy -- -D warnings
```

### Documentation

````rust
/// Represents a player in the game world.
///
/// This is the main entity type for human players.
///
/// # Examples
///
/// ```
/// let player = Player::new(0, "Steve".to_string());
/// assert_eq!(player.name, "Steve");
/// ```
///
/// # Panics
///
/// Panics if the player ID is invalid.
pub struct Player {
    pub id: u32,
    pub name: String,
}

impl Player {
    /// Creates a new player with the given ID and name.
    ///
    /// # Arguments
    ///
    /// * `id` - Unique player identifier
    /// * `name` - Display name
    pub fn new(id: u32, name: String) -> Self {
        Self { id, name }
    }
}
````

Generate and view docs:

```bash
cargo doc --no-deps --open
```

## Debugging

### Using println! and dbg!

```rust
// Simple debug output
println!("Value: {:?}", value);

// Macro that shows variable name
let x = 42;
dbg!(x);  // prints: [path:line] x = 42

// Chain debug calls
let result = some_function()
    .map(|x| { dbg!(x); x });
```

### Using a Debugger

```bash
# Using LLDB (macOS)
lldb target/debug/void
(lldb) b main
(lldb) run
(lldb) p variable_name

# Using GDB (Linux)
gdb target/debug/void
(gdb) break main
(gdb) run
(gdb) print variable_name
```

### Logging

```rust
// Structured logging (optional, requires log crate)
log::debug!("Processing packet: {:?}", packet);
log::warn!("Slow operation took {:?}", duration);
log::error!("Connection failed: {}", error);
```

## Performance Optimization Workflow

1. **Establish baseline**:

```bash
cargo bench --bench '*'
```

2. **Profile the code**:

```bash
cargo flamegraph
```

3. **Identify hotspots** and optimize

4. **Verify improvement**:

```bash
cargo bench --bench '*'
```

5. **Document changes** with rationale

## Git Workflow

### Setup

```bash
# Configure user
git config user.name "Your Name"
git config user.email "your@email.com"

# Useful aliases
git config --global alias.co checkout
git config --global alias.br branch
git config --global alias.ci commit
git config --global alias.st status
```

### Daily Workflow

```bash
# Update local repo
git fetch upstream
git rebase upstream/main

# Create feature branch
git checkout -b feature/my-feature

# Make changes, commit frequently
git add file.rs
git commit -m "feat: add new functionality"

# Push and create PR
git push origin feature/my-feature
```

### Useful Commands

```bash
# See what changed
git diff
git diff --staged

# Review commits before push
git log --oneline -5

# Amend last commit
git commit --amend

# Revert a commit
git revert <commit-hash>

# Clean up local branches
git branch -d feature/old-feature
```

## Building for Release

```bash
# Create release build
cargo build --release

# Size optimization
cargo build --release --strip

# Check binary size
ls -lh target/release/void
```

## Useful Commands Reference

```bash
# Clean build artifacts
cargo clean

# Check for errors without building
cargo check

# Run all tests with coverage
cargo tarpaulin --out Html

# Generate dependency graph
cargo tree

# Update dependencies
cargo update

# Check for outdated dependencies
cargo outdated

# Audit for security vulnerabilities
cargo audit
```

Happy coding! 🦀
