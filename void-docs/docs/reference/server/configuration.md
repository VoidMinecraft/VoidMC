# Server Configuration

## ServerConfigBuilder

`ServerConfigBuilder` provides a fluent API for constructing a `ServerConfig`:

```rust
use voidmc::{FrameLimits, ObservabilityConfig, ServerConfigBuilder, SpawnPosition};

let config = ServerConfigBuilder::new()
    .address("0.0.0.0:25565")
    .tick_rate(20)
    .max_players(100)
    .view_distance(10)
    .simulation_distance(10)
    .game_mode(1)
    .spawn_position(SpawnPosition { x: 0.0, z: 0.0, y: None })
    .spawn_chunk_radius(10)
    .initial_chunk_radius(3)
    .motd("My Void Server")
    .hardcore(false)
    .frame_limits(FrameLimits::default())
    .compression_threshold(Some(256))
    .observability(ObservabilityConfig::localhost(9464))
    .world_generator(MyGenerator::new())
    .configure_registries(|registries| {
        // Modify registry data before server starts
    })
    .build();
```

## ServerConfig Fields

| Field | Type | Default | Description |
|---|---|---|---|
| `address` | `String` | `"127.0.0.1:25565"` | TCP bind address |
| `tick_rate` | `u64` | `20` | Game ticks per second; must be at least `1` (`0` is read as `1`) |
| `max_players` | `i32` | `100` | Maximum player count (shown in server list) |
| `view_distance` | `i32` | `10` | Maximum server-side view distance (in chunks) |
| `simulation_distance` | `i32` | `10` | Simulation distance sent to clients |
| `game_mode` | `u8` | `1` (Creative) | Game mode (0=Survival, 1=Creative, 2=Adventure, 3=Spectator) every player joins in and whose rules the server applies; [`/gamemode`](../gameplay/commands.md#gamemode) only changes what a client shows |
| `spawn_position` | `SpawnPosition` | `{ x: 0.0, z: 0.0, y: None }` | World spawn location |
| `spawn_chunk_radius` | `i32` | `10` | Radius of pre-generated chunks around spawn |
| `initial_chunk_radius` | `i32` | `3` | Chunks sent to players during join (before streaming takes over), capped by the player's effective view distance |
| `motd` | `String` | `"Welcome to Void Server!"` | Message of the day (shown in server list) |
| `hardcore` | `bool` | `false` | Hardcore mode flag |
| `metrics_debug` | `bool` | `false` | Enable TPS metrics collection and file output |
| `metrics_tps_output` | `Option<String>` | `None` | Optional TPS CSV output path (defaults to `logs/tps-<timestamp>.csv`) |
| `max_packets_per_tick` | `usize` | `1000` | Cap the number of packets drained from the incoming channel each tick (0 = unlimited) |
| `packet_ingest_budget_ms` | `u64` | `4` | Time budget in milliseconds for packet ingest per tick (0 = unlimited) |
| `max_chunk_generations_per_tick` | `usize` | `8` | Cap the number of new chunks generated per tick (0 = unlimited) |
| `slow_tick_ms` | `u64` | `200` | Log a warning when a tick exceeds this duration (ms) |
| `frame_limits` | `FrameLimits` | See below | Bounds inbound/outbound packet sizes and nested decode resources |
| `compression_threshold` | `Option<u32>` | `Some(256)` | Packets of at least this many bytes are zlib-compressed after login; `None` disables compression (vanilla's negative threshold) |
| `observability` | `Option<ObservabilityConfig>` | `None` | Serves `/health` and Prometheus `/metrics` over HTTP; see [Health and Metrics Endpoints](./observability.md) |
| `world_generator` | `Box<dyn WorldGenerator>` | `DefaultWorldGenerator` | Terrain generation implementation |
| `registries` | `RegistryDataStore` | `RegistryDataStore::default()` | Minecraft registry data sent during configuration |
| `execution_mode` | `ExecutionMode` | `SingleThreaded` | Run systems and independent regions concurrently (`MultiThreaded`) or one after another on the game thread; see [Execution mode](#execution-mode) |

## Listen address

`VoidServer::run` binds `address` before starting the game loop and panics if
the address cannot be bound. The address actually bound is available to systems
as the `ListenAddress` resource, which is how a server configured with port `0`
learns the ephemeral port the operating system picked:

```rust
use bevy_ecs::prelude::Res;
use voidmc::ListenAddress;

fn announce(address: Res<ListenAddress>) {
    tracing::info!("listening on {}", address.0);
}
```

## Packet limits

`FrameLimits` rejects invalid packet lengths before allocation and carries the
limits used while decoding strings, collections, remaining-byte fields, and NBT.
The production defaults allow 2 MiB inbound frames and 8 MiB outbound frames.
Applications can replace the complete policy through
`ServerConfigBuilder::frame_limits`.

With compression enabled, a compressed inbound frame is rejected when its
declared uncompressed length is below the threshold, above
`min(max_inbound_frame_bytes, 8 MiB)`, or does not match the inflated payload.
Inflation never writes past the declared length. `max_outbound_frame_bytes`
applies to the packet before compression, and once a connection is compressed
it is capped at 8 MiB, the largest uncompressed size the vanilla client
accepts.

## Compression

`compression_threshold` mirrors vanilla's `network-compression-threshold`.
The network thread sends Set Compression right before Login Success, then
frames every later packet in both directions as
`VarInt packet_length | VarInt data_length | payload`: packets smaller than the
threshold travel with `data_length = 0` and an uncompressed payload, larger ones
are zlib-compressed. Status and ping connections are never compressed.
Compression runs on the Tokio network thread, never on the game thread.

`Some(0)` compresses every packet. On a LAN where bandwidth is plentiful,
`None` saves the compression CPU; over residential uplinks keep it enabled: a
view-distance-12 join is ~37 MB uncompressed and ~0.42 MB compressed.

Decoded packets must consume their complete declared frame. Packet definitions
that intentionally accept an opaque tail must mark that field with
`#[codec(remaining)]`.

## SpawnPosition

```rust
pub struct SpawnPosition {
    pub x: f64,
    pub z: f64,
    pub y: Option<f64>,
}
```

When `y` is `None` (the default), the server uses `WorldGenerator::surface_height_at(x.floor(), z.floor())` of the configured `world_generator` as the spawn Y coordinate: the first air block above the terrain and any water, so players spawn standing on it. When `y` is `Some`, that height is used as is.

## Execution mode

```rust
use voidmc::{ExecutionMode, ServerConfigBuilder};

let config = ServerConfigBuilder::new()
    .execution_mode(ExecutionMode::MultiThreaded)
    .build();
```

| Mode | Systems | Region-partitioned work |
|---|---|---|
| `SingleThreaded` (default) | Every schedule uses Bevy's single-threaded executor: one system at a time, on the game thread | Regions one after another, on the game thread |
| `MultiThreaded` | Bevy's multi-threaded executor: systems whose data access does not conflict run at the same time on the compute task pool | Independent regions run as concurrent tasks on the compute task pool |

Both modes produce the same world state, provided that systems which write
data another system reads or writes are ordered (`.before` / `.after` or a
[`VoidSystems`](/reference/server/ecs) set). Under `MultiThreaded`, Bevy picks
the order of two unordered conflicting systems at run time. The order of
packets sent by two unordered systems to the same client is not fixed either.

VoidMC's own `Update` sets are not all ordered with each other yet: Bevy's
ambiguity detection reports unordered conflicting pairs among `CommandDrain`,
`ItemUseDrain`, `MenuClickDrain`, `TeleportBarrier`, `ItemPickup`,
`KeepAlive`, `EntitySimulation` and, with `RedstonePlugin`, `Redstone`.
Their order within a tick can differ from the single-threaded one, which
only shows when two of them act on the same thing in the same tick, for
example an item used on the tick another item is picked up into the same
inventory.

The compute task pool always has at least two threads, so two systems or
regions can be in flight at once on a two-core machine. The running mode is
available as the `ExecutionMode` resource. See
[Parallel execution](/reference/server/architecture#parallel-execution) for
what is partitioned by region.

## ServerConfigResource

At runtime, a `ServerConfigResource` is inserted into the Bevy world as a plain-data ECS resource (no `Box<dyn>` fields). Systems and command handlers can read it:

```rust
fn my_system(config: Res<ServerConfigResource>) {
    println!("MOTD: {}", config.motd);
    println!("Spawn: {}, {}", config.spawn_x, config.spawn_z);
}
```

Fields mirror `ServerConfig` except that `SpawnPosition` is flattened into `spawn_x`, `spawn_z`, and `spawn_y: Option<f64>`, and the world generator and registries are stored as separate resources (`WorldGen` and `RegistryDataStore`).
