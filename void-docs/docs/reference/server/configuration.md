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
| `max_packets_per_tick` | `usize` | `1000` | Cap packets decoded and dispatched each tick; must be positive |
| `packet_ingest_budget_ms` | `u64` | `4` | Time budget in milliseconds for staging, decoding, and dispatch; must be positive |
| `max_chunk_generations_per_tick` | `usize` | `8` | Cap the number of new chunks generated per tick (0 = unlimited) |
| `slow_tick_ms` | `u64` | `200` | Log a warning when a tick exceeds this duration (ms) |
| `frame_limits` | `FrameLimits` | See below | Bounds inbound/outbound packet sizes and nested decode resources |
| `world_generator` | `Box<dyn WorldGenerator>` | `DefaultWorldGenerator` | Terrain generation implementation |
| `registries` | `RegistryDataStore` | `RegistryDataStore::default()` | Minecraft registry data sent during configuration |

## Packet limits

`FrameLimits` rejects invalid packet lengths before allocation and carries the
limits used while decoding strings, collections, remaining-byte fields, and NBT.
The production defaults allow 2 MiB inbound frames and 8 MiB outbound frames.
Applications can replace the complete policy through
`ServerConfigBuilder::frame_limits`.

The network accepts at most 256 concurrent TCP connections. A connection may
queue 32 inbound frames or 4 MiB of inbound frame bodies; all connections
share a 64 MiB inbound budget. Outbound queues allow at most 16,384 packets
or 8 MiB of serialized payload per connection, with a 128 MiB global payload
budget. Exceeding an inbound limit disconnects that client. Outbound sends
return `SendError::Overloaded` through `Players::try_send` or
`WorldPlayers::try_send` and request an overload disconnect. `send` remains a
convenience method for callers that do not need the result. These budgets
cover queued packet payloads; socket buffers, decoded game state, and Rust
container overhead add to process memory. Direct outbound items retain both
their protocol value and a cached serialized frame.
The ordered event channel and game-thread staging queue each hold at most
16,384 events. The close control
channel at most 257 commands, and each connection's close channel one request.
Optional disconnect packets and status responses are each limited to 64 KiB.
The status response channel holds one item. The production fallback outgoing
channel holds one item and has no receiver.

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
