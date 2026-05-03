# Server Configuration

## ServerBuilder

`ServerBuilder` provides a fluent API for constructing a `ServerConfig`:

```rust
use voidmc::{ServerBuilder, SpawnPosition};

let config = ServerBuilder::new()
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
| `world_generator` | `Box<dyn WorldGenerator>` | `DefaultWorldGenerator` | Terrain generation implementation |
| `registries` | `RegistryDataStore` | `RegistryDataStore::default()` | Minecraft registry data sent during configuration |

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
