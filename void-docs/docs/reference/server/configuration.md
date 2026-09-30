# Server Configuration

## ServerConfigBuilder

`ServerConfigBuilder` provides a fluent API for constructing a `ServerConfig`:

```rust
use voidmc::{FrameLimits, ServerConfigBuilder, SpawnPosition};

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
| `tick_rate` | `u64` | `20` | Game ticks per second |
| `max_players` | `i32` | `100` | Maximum player count (shown in server list) |
| `view_distance` | `i32` | `10` | Maximum server-side view distance (in chunks) |
| `simulation_distance` | `i32` | `10` | Simulation distance sent to clients |
| `game_mode` | `u8` | `1` (Creative) | Default game mode (0=Survival, 1=Creative, 2=Adventure, 3=Spectator) |
| `spawn_position` | `SpawnPosition` | `{ x: 0.0, z: 0.0, y: None }` | World spawn location |
| `spawn_chunk_radius` | `i32` | `10` | Radius of pre-generated chunks around spawn |
| `initial_chunk_radius` | `i32` | `3` | Chunks sent to players during join (before streaming takes over) |
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
| `world_generator` | `Box<dyn WorldGenerator>` | `DefaultWorldGenerator` | Terrain generation implementation |
| `registries` | `RegistryDataStore` | `RegistryDataStore::default()` | Minecraft registry data sent during configuration |

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

When `y` is `None` (the default), the server automatically computes the spawn Y coordinate by calling `WorldGenerator::surface_height_at(x, z) + 1`. This ensures players always spawn on top of the terrain.

## ServerConfigResource

At runtime, a `ServerConfigResource` is inserted into the Bevy world as a plain-data ECS resource (no `Box<dyn>` fields). Systems and command handlers can read it:

```rust
fn my_system(config: Res<ServerConfigResource>) {
    println!("MOTD: {}", config.motd);
    println!("Spawn: {}, {}", config.spawn_x, config.spawn_z);
}
```

Fields mirror `ServerConfig` except that `SpawnPosition` is flattened into `spawn_x`, `spawn_z`, and `spawn_y: Option<f64>`, and the world generator and registries are stored as separate resources (`WorldGen` and `RegistryDataStore`).
