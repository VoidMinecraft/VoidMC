# Architecture

## Dual-Threaded Model

Void runs on two threads:

1. **Network thread** — A Tokio multi-threaded runtime that handles TCP connections, packet I/O, and per-client async tasks.
2. **Game thread** — A Bevy ECS application that runs the game loop, processes packets, updates world state, and sends responses.

The two threads communicate exclusively through [flume](https://docs.rs/flume) channels:

| Channel | Direction | Type | Purpose |
|---|---|---|---|
| `events` | Network -> Game | `ConnectionEvent` | Ordered connection, packet, and disconnection events |
| per-client outbound | Game -> Network | `OutgoingPacket` | One bounded queue per client, written to directly by the send path |
| `control` | Game -> Network | `ConnectionCommand` | Typed close and shutdown requests |

```
+-----------------------------+       flume channels       +------------------------------+
|       Network Thread        | <-- per-client outbound, -- |        Game Thread            |
|   (Tokio multi-threaded)    |     control                 |   (Bevy ECS tick loop)       |
|                             | --> ordered events           |                              |
|  Server::run()              |                             |  App::run()                  |
|   +- accept TCP connections |                             |   +- PreUpdate: ingest packets|
|   +- spawn Client tasks     |                             |   +- Update: keep-alive       |
|   +- handle close requests  |                             |   +- PostUpdate: broadcast    |
|                             |                             |   +- Observers: events        |
+-----------------------------+                             +------------------------------+
```

There is no global outgoing channel. On accept, the network thread creates a
bounded outbound queue for the connection (`OUTBOUND_QUEUE_CAPACITY`, 16384
packets) and announces a `ConnectionHandle` through `Connected` before the
client task starts. The game thread creates the entity on `Connected`, before
the first packet. `ClientSenders` keeps handles keyed by client entity,
and `Players` / `WorldPlayers` push straight into the target client's queue
with a non-blocking `try_send`.

If a queue is full the client is not keeping up; the game thread never blocks
and never drops individual packets (which would desync the client). Instead it
requests an idempotent close with reason `Overloaded`. The network server gives
the task until its close deadline, then aborts it if necessary. A final
`Disconnected` event follows task completion and the entity is despawned.

`Server` owns the listener, task set, ID allocator, and task handles. Each
connection task owns its socket and reader/writer halves. Bevy owns protocol
phase in `ConnectionState`; the status fast path remains an exception. The
connection task retains the peer address for logging. The server reports one
`Disconnected { reason }` event after joining a task, including task failures.
IDs stop allocating at `u32::MAX` instead of wrapping. Whole-server runtime
shutdown is covered separately by NET-014.

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
| **PreUpdate** | `ingest_network_packets` | Drain ordered lifecycle events, decode packets, dispatch to handlers |
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
ClientReader::receive()         -- Persistent network reader future
  |
  v
ConnectionEvent::Packet(IncomingPacket { client_id, raw_packet })
  |                             -- ordered flume lifecycle stream
  v
ingest_network_packets()        -- Game thread (PreUpdate)
  |
  +- Look up entity created by ConnectionEvent::Connected
  +- Read ConnectionState component
  +- Decode packet based on state
  |
  v
world.trigger(PacketEvent<T>) + world.flush()
  |
  v
On<PacketEvent<T>> observers    -- Plugins (handshake, login, play, ...)
  |
  +- Update ECS components
  +- Reply through `Players::send(entity, packet)`
  +- Trigger semantic events (commands.trigger)
  |
  v
Semantic observers              -- Triggered by events
  |
  +- on_player_ready: broadcast spawn to other players
  +- on_player_quit: broadcast removal
  |
  v
PostUpdate systems              -- Same tick (see VoidSystems sets)
  |
  +- broadcast_position: delta-encoded movement
  +- stream_chunks: load/unload chunks by view distance
  |
  v
Players / WorldPlayers          -- voidmc::players (Entity -> ClientSenders)
  |
  v
OutgoingPacket { client_id, packet }
  |                             -- that client's bounded flume channel (try_send)
  v
Client::run()                   -- Network thread
  |
  +- ClientWriter::send()       -- Single serialized writer future, buffered
  +- ClientWriter::flush()      -- Once per drained batch
  |
  v
Client (TCP)
```

`ClientSocket` is consumed into independently owned `ClientReader` and
`ClientWriter` halves. `Client::run` keeps one reader loop and one writer loop
alive for the connection lifetime. Outbound readiness therefore cannot cancel
an inbound frame after part of its length prefix or body has been consumed, and
only the writer half can write, so partially written frames cannot interleave.

`ClientWriter::send` frames the packet into one reusable buffer (length prefix
and body in a single `write_all`) and hands it to a `BufWriter`; `send` may
leave the frame buffered, and `ClientWriter::flush` is required to guarantee
delivery. The writer loop drains every packet already queued on the client's
channels into the same batch, then flushes once, so a tick's worth of packets
leaves as a single write. A caller that drives `ClientWriter` directly must
call `flush` itself. The frame buffer is retained between sends up to
`max_outbound_frame_bytes / 8` (1 MiB by default, enough for a lit chunk
packet); a larger frame is released after it is written. If a packet is
rejected mid-batch (for example
`FrameTooLarge`), the frames accepted before it are flushed before the error
is propagated.
