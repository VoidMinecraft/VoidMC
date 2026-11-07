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
| `max_packets_per_tick` | `usize` | `1000` | Cap the number of packets drained from the incoming channel each tick (0 = unlimited) |
| `packet_ingest_budget_ms` | `u64` | `4` | Time budget in milliseconds for packet ingest per tick (0 = unlimited) |
| `max_chunk_generations_per_tick` | `usize` | `8` | Cap the number of new chunks generated per tick (0 = unlimited) |
| `slow_tick_ms` | `u64` | `200` | Log a warning when a tick exceeds this duration (ms) |
| `frame_limits` | `FrameLimits` | See below | Bounds inbound/outbound packet sizes and nested decode resources |
| `step_socket` | `Option<PathBuf>` | `None` | Unix socket that drives the server tick by tick. See [Stepped server](#stepped-server) |
| `world_generator` | `Box<dyn WorldGenerator>` | `DefaultWorldGenerator` | Terrain generation implementation |
| `registries` | `RegistryDataStore` | `RegistryDataStore::default()` | Minecraft registry data sent during configuration |

## Stepped server

`ServerConfigBuilder::step_socket(path)` replaces the tick loop. The server starts locked, runs no
tick by itself, and takes one order per line on the socket. Each order gets one JSON line back with
`tick`, `sent` and `received`:

| Order | Effect |
|---|---|
| `step <count> [received]` | Waits until `received` packets have arrived, then runs `count` ticks. After 10 s it answers `"ok":false` and runs nothing |
| `status` | Reports the counters |
| `stop` | Writes `AppExit`, runs one last tick so that systems see it, and ends the server |

`sent` and `received` count packets to and from all clients, so the barrier holds for one client.
Without the option the server keeps its normal loop, and the only addition is one relaxed load per
packet sent.

### Time and chance

What is guaranteed on a stepped server: nothing the engine, `void-ui` or `void-map` sends reads the
wall clock or fresh entropy. Their time comes from the ticks, their chance and their UUIDs from
generators with a fixed seed, and every schedule runs its systems one at a time, in the same order
on every run. Bevy already does so unless its `multi_threaded` feature is on. The stepped mode sets
it anyway on every schedule that exists once the app is finished, so a crate that turns the feature
on does not change their order; a schedule created later keeps Bevy's default. The one exception is
the world border, whose transitions still run on the wall clock. Clippy holds this rule, as
described at the end of this section.

What is not guaranteed in general: the same packets in the same order on every run. Some hashed
collections still decide an order: a hashed set or map iterated before sending can order packets
differently from one process to the next. The sets of chunks a player holds (`ChunkSet`) and the
block entities of a chunk are fixed for that. On `/map` and on `void-example`, two stepped runs
that receive the same packets at the same ticks give an empty `voidc trace diff`.

`SteppedPacePlugin`, added by the stepped runner, is the one thing that switches them. It sets two
resources that only it can build:

- `voidmc::pace::Pace`, the time. Read it with the `Clock` system parameter, or with
  `pace::of(world)`. Without it, the time is the wall clock.
- `voidmc::pace::Dice`, the generator, with a fixed seed. Draw from it with the `Chance` system
  parameter, or with `pace::roll(world, ..)`. Without it, the draw uses the thread's generator.

Like any resource, both can still be removed, or replaced by a copy taken from another world. A
plugin that does so runs on the wall clock or on fresh entropy again, and gives up the guarantee.

| Reading | Without the plugin | Stepped |
|---------|--------------------|---------|
| `now_millis()` | Milliseconds since the Unix epoch, for dates | Ticks played × tick length |
| `elapsed_millis()` | Milliseconds on a monotonic clock, for durations | Ticks played × tick length |
| `deadline(budget).passed()` | Once `budget` has gone by | Never: work is bounded by counts alone |
| `roll(..)` | The thread's generator | The generator with the fixed seed |

These go through it: the keep-alive id and the latency in the tab list (`elapsed_millis`), the date
of a toast (`now_millis`), new entity and boss bar UUIDs, the packet ingest budget, the wander of
mobs, `summon --wander`, `@r`, and the chunk deadlines of `void-map`. The sets of chunks a player
holds (`ChunkSet`) iterate in the same order in every process, so the chunks a player leaves are
unloaded in the same order.

Clippy holds the rule: `clippy.toml` forbids `Instant::now`, `SystemTime::now`, `rand::thread_rng`,
`rand::random` and `uuid::Uuid::new_v4`. Each use that stays, in logs, measures, tests or the world
border, carries an `#[expect(clippy::disallowed_methods, reason = "..")]` that says why.

The runner blocks on the socket, so this mode leaves out `TerminalCtrlCHandlerPlugin`: Ctrl-C ends
the process at once, without `AppExit`. The server refuses a socket that another server answers on,
and exits with an error when it cannot bind it.

`voidc step` drives this socket together with a real client: see `void-clients/README.md`.

## Packet limits

`FrameLimits` rejects invalid packet lengths before allocation and carries the
limits used while decoding strings, collections, remaining-byte fields, and NBT.
The production defaults allow 2 MiB inbound frames and 8 MiB outbound frames.
Applications can replace the complete policy through
`ServerConfigBuilder::frame_limits`.

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
