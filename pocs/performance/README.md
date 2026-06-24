# Performance POCs

This folder contains small reproducible tools used as performance evidence for
VoidMC. They are intentionally kept outside the main workspace so the server
crates do not inherit benchmark-only dependencies.

## TCP connect stress

Start the example server:

```bash
cargo run --release -p voidmc-example
```

Run the stress tool from another shell:

```bash
cargo run --manifest-path pocs/performance/Cargo.toml --release --bin tcp_connect_stress -- --addr 127.0.0.1:25565 --clients 64 --timeout-ms 1000
```

The tool measures TCP accept responsiveness only. It does not perform the full
Minecraft handshake, login, encryption, or play-state transition.
