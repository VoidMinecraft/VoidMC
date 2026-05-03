# Getting Started

Welcome to Void, a high-performance Minecraft server implementation built in Rust. This guide will help you get up and running with the project.

## What is Void?

Void is an academic project (EIP) that implements a Minecraft-compatible server from scratch. It demonstrates:

- **Advanced network protocol implementation** (Minecraft 26.1.2)
- **High-performance async architecture** using Tokio
- **Modular design** with clear separation between protocol, codec, and networking layers
- **Production-ready practices** including comprehensive testing and documentation

## Quick Start

### Prerequisites

- **Rust**: 1.70+ (install from [rustup.rs](https://rustup.rs))
- **Cargo**: Comes with Rust
- **Git**: For cloning the repository

### Installation

```bash
# Clone the repository
git clone https://github.com/void-minecraft/void.git
cd void

# Build the project
cargo build --release

# Run the server
cargo run --release
```

The server will start on `127.0.0.1:25565` by default.

### First Connection

Once the server is running, you can connect with any Minecraft client (26.1.2):

1. Open Minecraft
2. Add a server with address `127.0.0.1`
3. Connect and enjoy!

## Project Structure

```
void/                          # Main server application
void-protocol/                 # Minecraft protocol definitions
void-codec/                    # Binary encoding/decoding
void-codec-macros/            # Procedural macros for codec
void-net/                      # Networking layer
void-docs/                     # Documentation site (this)
```

## Next Steps

- **Learn the Architecture**: Check the [Architecture guide](/architecture/)
- **Want to Contribute?**: Read [Contributing guidelines](/contributing/)
- **Performance Curious?**: See [Performance metrics](/performance/)
- **Building a Feature?**: Check the inline Rust documentation with `cargo doc --open`

## Key Concepts

### Protocol States

Minecraft connections flow through several states:

- **Handshake** → **Status** → **Login** → **Configuration** → **Play**

Each state has specific packets that can be sent/received.

### Async-First Design

All I/O operations are non-blocking using Tokio, enabling thousands of concurrent connections.

### Type Safety

Rust's type system ensures packet definitions are correct at compile time, preventing many runtime errors.
