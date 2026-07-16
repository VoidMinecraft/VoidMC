# Configuration Examples

VoidMC configuration is currently code-first through `ServerConfigBuilder`. The example server also reads a small set of environment variables for diagnostics and metrics.

## Development Configuration

```rust
use voidmc::{ServerConfigBuilder, VoidServer};

fn main() {
    let config = ServerConfigBuilder::new()
        .address("127.0.0.1:25565")
        .tick_rate(20)
        .max_players(20)
        .view_distance(8)
        .simulation_distance(8)
        .spawn_chunk_radius(4)
        .initial_chunk_radius(4)
        .motd("VoidMC dev server")
        .metrics_debug(true)
        .metrics_tps_output("logs/tps-dev.csv")
        .build();

    VoidServer::new(config).run();
}
```

Use this shape for local development because it keeps startup fast and enables TPS evidence.

## Demo/Review Configuration

```rust
use voidmc::{ServerConfigBuilder, VoidServer};

fn main() {
    let config = ServerConfigBuilder::new()
        .address("0.0.0.0:25565")
        .tick_rate(20)
        .max_players(100)
        .view_distance(10)
        .simulation_distance(10)
        .max_packets_per_tick(1000)
        .packet_ingest_budget_ms(4)
        .max_chunk_generations_per_tick(8)
        .slow_tick_ms(200)
        .motd("VoidMC review server")
        .build();

    VoidServer::new(config).run();
}
```

This shape mirrors the defaults while making the reliability budgets visible.

## Example Server Environment

`void-example` reads these variables at startup:

| Variable | Example | Purpose |
|---|---|---|
| `RUST_LOG` | `info` | Controls tracing filters. |
| `VOID_METRICS_DEBUG` | `1` | Enables TPS collection. |
| `VOID_TPS_OUTPUT` | `logs/tps-demo.csv` | Selects TPS CSV output. |
| `VOID_METRICS_MODE` | `flame` | Enables flame trace mode. |
| `VOID_FLAME_OUTPUT` | `logs/trace.folded` | Selects flame trace output. |
| `VOID_FLAME_INCLUDE_IDLE` | `1` | Includes idle time to show server utilization. |
| `VOID_PACKET_DEBUG` | `1` | Adds network packet debug logging. |

## Configuration Reference

See [Server Configuration](/reference/server/configuration) for all fields and defaults.
