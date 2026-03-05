# Architecture

## Dual-Threaded Model

Void runs on two threads:

1. **Network thread** — A Tokio multi-threaded runtime that handles TCP connections, packet I/O, and per-client async tasks.
2. **Game thread** — A Bevy ECS application that runs the game loop, processes packets, updates world state, and sends responses.

The two threads communicate exclusively through [flume](https://docs.rs/flume) channels:

| Channel | Direction | Type | Purpose |
|---|---|---|---|
| `incoming` | Network -> Game | `IncomingPacket` | Raw packets received from clients |
| `outgoing` | Game -> Network | `OutgoingPacket` | Encoded packets to send to clients |
| `disconnect` | Network -> Game | `u32` (client ID) | Client disconnection notifications |
| `kick` | Game -> Network | `u32` (client ID) | Server-initiated kick requests |

```
+-----------------------------+       flume channels       +------------------------------+
|       Network Thread        | <---- outgoing, kick ----- |        Game Thread            |
|   (Tokio multi-threaded)    | -----> incoming, disconnect |   (Bevy ECS tick loop)       |
|                             |                             |                              |
|  Server::run()              |                             |  App::run()                  |
|   +- accept TCP connections |                             |   +- PreUpdate: ingest packets|
|   +- spawn Client tasks     |                             |   +- Update: keep-alive       |
|   +- route outgoing packets |                             |   +- PostUpdate: broadcast    |
|   +- handle kick requests   |                             |   +- Observers: events        |
+-----------------------------+                             +------------------------------+
```

## Tick Loop

The game thread runs a fixed-rate tick loop using Bevy's `ScheduleRunnerPlugin`. The tick rate is configurable (default: **20 TPS**, or 50ms per tick).

```rust
ScheduleRunnerPlugin::run_loop(Duration::from_millis(1000 / tick_rate))
```

## Bevy Schedule Organization

Each tick executes these schedules in order:

| Schedule | Systems | Purpose |
|---|---|---|
| **Startup** | `init_world` | Pre-generate spawn area chunks |
| **PreUpdate** | `ingest_network_packets` | Drain incoming channel, decode packets, dispatch to handlers |
| **Update** | `send_keep_alive` | Periodic keep-alive packets |
| **PostUpdate** | `broadcast_position`, `update_previous_positions`, `stream_chunks` | Sync player movement, load/unload chunks |

Handlers are called directly from `ingest_network_packets` (not as separate systems) because they need exclusive `&mut World` access for entity mutation.

## Plugin System

`VoidServer` exposes two extension points:

### `add_plugin`

Register a closure that receives `&mut App` to add custom systems, resources, or observers:

```rust
VoidServer::new(config)
    .add_plugin(|app| {
        app.add_systems(Update, my_custom_system);
        app.insert_resource(MyResource::new());
    })
```

Plugins are applied before the Bevy app starts but after all core plugins are registered. This means plugins can modify resources like `RegistryDataStore` before the first tick.

### `add_command`

Register a command built with `CommandBuilder` directly on the server:

```rust
VoidServer::new(config)
    .add_command(
        CommandBuilder::new("hello")
            .description("Say hello")
            .handler(|ctx| ctx.reply("Hello!"))
            .build(),
    )
```

Commands added this way are inserted into the `CommandRegistry` resource after plugins are applied.

## Packet Flow

The complete lifecycle of a packet through the system:

```
Client (TCP)
  |
  v
ClientSocket::receive()         -- Network thread
  |
  v
IncomingPacket { client_id, raw_packet }
  |                             -- flume channel
  v
ingest_network_packets()        -- Game thread (PreUpdate)
  |
  +- Look up or spawn client Entity
  +- Read ConnectionState component
  +- Decode packet based on state
  |
  v
handle_{state}_packet()         -- Direct function call
  |
  +- Update ECS components
  +- Send response via outgoing channel
  +- Trigger semantic events (world.trigger + world.flush)
  |
  v
Observers                       -- Triggered by events
  |
  +- on_player_ready: broadcast spawn to other players
  +- on_player_quit: broadcast removal
  |
  v
PostUpdate systems              -- Same tick
  |
  +- broadcast_position: delta-encoded movement
  +- stream_chunks: load/unload chunks by view distance
  |
  v
OutgoingPacket { client_id, packet }
  |                             -- flume channel
  v
Server::run()                   -- Network thread
  |
  +- Route to per-client channel
  |
  v
Client::run()
  |
  +- ClientSocket::send()       -- Encode + write to TCP
  |
  v
Client (TCP)
```
