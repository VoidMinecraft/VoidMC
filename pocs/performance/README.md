# Performance POCs

This folder contains small reproducible tools used as performance evidence for
VoidMC. They are intentionally kept outside the main workspace so the server
crates do not inherit benchmark-only dependencies.

## Automation

The repository's **Benchmarks** GitHub Actions workflow runs the workspace's
Criterion benchmarks weekly and on demand, then uploads their HTML reports as
an artifact. It may also run the TCP connect stress POC when explicitly
selected during manual dispatch. These runs produce comparative evidence, not
pass/fail performance thresholds, because hosted runners have variable load.

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
