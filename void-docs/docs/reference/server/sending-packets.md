# Sending Packets & System Sets

## Sending packets

Clientbound packets are sent through `voidmc::Players` (a `SystemParam`) or
`voidmc::WorldPlayers` (for code that holds a `&World`). Both address players
by `Entity` and accept any packet struct from `voidmc_protocol::clientbound`
(everything implements `Into<ClientboundPacket>`).

```rust
use voidmc::{Players, DimensionId, ChunkPos};
use voidmc_protocol::clientbound::{KeepAlive, SystemChat};

fn my_system(players: Players, me: Query<Entity, With<Operator>>) {
    let me = me.single().unwrap();

    players.send(me, KeepAlive { keep_alive_id: 1 });       // one client
    players.broadcast(SystemChat { content, overlay: false });  // every ready player
    players.broadcast_except(me, packet);                   // everyone but one
    players.broadcast_chunk(DimensionId::Overworld, ChunkPos::new(0, 0), packet);

    players.ready()                                         // composable form
        .in_dimension(DimensionId::Nether)
        .except(me)
        .filter(|r| r.client_id() % 2 == 0)
        .send(packet);
}
```

`ready()` snapshots the ready players into a `Vec`. Take it **once per system
run** and use the borrowing variants inside loops:

```rust
fn broadcast_health(players: Players, mobs: Query<(&MinecraftEntityId, &Health, Option<&EntityDimension>)>) {
    let ready = players.ready();
    for (id, health, dim) in mobs.iter() {
        let dim = dim.map(|d| d.0);
        ready.send_where(|r| r.visible_from(dim), health_packet(id.0, health));
        ready.send_except(some_entity, other_packet);
    }
}
```

| Method | Recipients |
|---|---|
| `send(entity, packet)` | One client entity — ready or still in status/login/configuration. |
| `send_to(entities, packet)` | Each listed entity. |
| `broadcast(packet)` | Every player with `PlayerReady`. |
| `broadcast_except(entity, packet)` | Every ready player but one. |
| `broadcast_chunk(dimension, chunk, packet)` | Ready players whose `LoadedChunks` contains the chunk. |
| `ready()` → `Recipients` | Snapshot of ready players; chain `except`, `in_dimension`, `visible_from(Option<DimensionId>)`, `seeing_chunk`, `filter`, then `send`. |
| `Recipients::send_except(entity, packet)` / `send_where(pred, packet)` | Same as above without consuming or allocating — for loops. |

`Recipient` exposes `entity()`, `client_id()`, `dimension()`,
`visible_from(Option<DimensionId>)` and `sees_chunk(dimension, chunk)` to predicates.

### From exclusive-world code

Command handlers, item behaviours and drain systems hold a `&World`:

```rust
fn handle(ctx: &mut CommandContext) {
    ctx.players().send(ctx.entity, packet);           // CommandContext helper
    WorldPlayers::new(world).broadcast(packet);       // anywhere with &World
}
```

`WorldPlayers` needs no `&mut World` and no prior component registration.

### Failure handling

Nothing is silently dropped. Each failure is logged once, with the entity and client id:

| Situation | Level |
|---|---|
| Entity no longer exists (player left this tick) | `debug` |
| Entity exists but has no `ClientId` | `warn` |
| Outgoing channel closed (network thread gone) | `error`, once per process |

### Parameter conflicts

`Players` reads `ClientId`, `PlayerReady`, `PlayerDimension` and `LoadedChunks`.
A system that mutates one of those must put both in a `ParamSet`, otherwise
Bevy rejects it at startup (error B0001). Inside the framework only chunk
streaming does this.

### Raw layer

`NetworkChannels.outgoing` and `OutgoingPacket { client_id, packet }` remain
public for code that already holds a client id; prefer `Players`.

## System sets

`voidmc::VoidSystems` names every framework phase so user systems can be
ordered against them with `.before(..)` / `.after(..)`. Ordering only applies
inside one schedule.

| Set | Schedule | What runs there |
|---|---|---|
| `NetworkIngest` | `PreUpdate` | Drain incoming channel, spawn client entities, decode + dispatch (`On<PacketEvent<T>>` observers), handle disconnects. |
| `CommandDrain` | `Update` | Execute queued commands (`CommandSystems::DrainQueue` is inside it). |
| `ItemUseDrain` | `Update` | Run `ItemBehavior` handlers for queued uses/breaks. |
| `KeepAlive` | `Update` | Send `KeepAlive`. After `CommandDrain`. |
| `EntitySimulation` | `Update` | Settle spawns, wander, physics. After `KeepAlive`. |
| `ItemPickup` | `Update` | Pickup-delay ticking and item pickup. |
| `EntityBroadcast` | `PostUpdate` | Spawn / movement / motion / metadata packets for non-player entities. |
| `PlayerBroadcast` | `PostUpdate` | Other players' movement and head rotation. |
| `ChunkStreaming` | `PostUpdate` | Chunk load/unload packets; updates `LoadedChunks`. |
| `InventorySync` | `PostUpdate` | Resync players flagged `InventoryDirty`. |
| `StatusSnapshot` | `PostUpdate` | Refresh the status-response snapshot read by the network thread. |
| `Metrics` | `PostUpdate` | TPS tracking (only when `metrics_debug` is on). |

`CommandDrain → KeepAlive → EntitySimulation` are chained; the other sets in
a schedule are unordered relative to each other.

```rust
use bevy_app::{PostUpdate, Update};
use voidmc::VoidSystems;

app.add_systems(Update, apply_boss_bar_damage.after(VoidSystems::EntitySimulation))
   .add_systems(PostUpdate, send_boss_bars.after(VoidSystems::EntityBroadcast));
```

A system that reacts to a block change after players received `BlockUpdate`
belongs after `ChunkStreaming`; one that must see a freshly parsed packet in
the same tick goes in `Update` (after `NetworkIngest` by schedule order).
