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
    players.broadcast(SystemChat { content, overlay: false });  // every ready player (prefer `Messages`)
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

### Audiences

A feature that keeps addressing the same players over time (a boss bar, a
looping sound, a private entity) stores a `voidmc::Audience` instead of a
player list. It narrows a `Recipients` snapshot with `resolve` and tests one
`Recipient` with `includes`, so every plugin reuses the same filters instead of
inventing its own set.

| Variant | Players |
|---|---|
| `Audience::All` (default) | Every ready player. |
| `Audience::InDimension(dimension)` | Ready players in that dimension. |
| `Audience::Explicit(HashSet<Entity>)` — `Audience::explicit([a, b])` | Exactly these player entities, while ready. |
| `Audience::Custom(Arc<dyn Fn(&Recipient) -> bool>)` — `Audience::custom(\|r\| ..)` | Any predicate over `Recipient`. |

`audience.except(entity)` wraps any audience in a `Custom` that also skips
that entity.

```rust
use voidmc::{Audience, Players, DimensionId};

fn announce(players: Players, audience: Res<RaidAudience>) {
    audience.0.resolve(players.ready()).send(packet);
}

let nether_only = Audience::InDimension(DimensionId::Nether);
let party = Audience::explicit([alice, bob]);
let ops = Audience::custom(|r| r.client_id() < 10);
let others = Audience::InDimension(DimensionId::Nether).except(me);
```

### From exclusive-world code

Command handlers, item behaviours and drain systems hold a `&World`:

```rust
fn handle(ctx: &mut CommandContext) {
    ctx.players().send(ctx.entity, packet);           // CommandContext helper
    WorldPlayers::new(world).broadcast(packet);       // anywhere with &World
}
```

`WorldPlayers` needs no `&mut World` and no prior component registration.

### Higher-level send APIs

`Players` is the raw layer; each feature has a fire-and-forget request API on
top of it with the same `Audience` / `viewers` vocabulary:

| Content | API |
|---|---|
| Chat and action-bar text | [`Messages` / `WorldMessages`](../gameplay/messages.md) — the text-messaging entry point. |
| Particles | [`Particles` / `WorldParticles`](../gameplay/particles.md) |
| Sounds | [`Sounds` / `WorldSounds`](../gameplay/sounds.md) |
| Boss bars | [`BossBar` component](../gameplay/boss-bars.md) |

### Failure handling

Nothing is silently dropped. Each failure is logged once, with the entity and client id:

| Situation | Level |
|---|---|
| Entity no longer exists (player left this tick) | `debug` |
| Entity exists but has no `ClientId` | `warn` |
| Client's outbound queue full (client not draining) | `warn`, once per client; the client is kicked |
| Client's connection already closed (disconnect not yet ingested) | `debug` |
| Client entity has no outbound channel and the fallback channel is closed | `error`, once per process |

### Parameter conflicts

`Players` reads `ClientId`, `PlayerReady`, `PlayerDimension` and `LoadedChunks`.
A system that mutates one of those must put both in a `ParamSet`, otherwise
Bevy rejects it at startup (error B0001). Inside the framework only chunk
streaming does this.

### Raw layer

Every client has its own bounded outbound queue (`OUTBOUND_QUEUE_CAPACITY`)
held in `ClientSenders`; sends are non-blocking `try_send`s and never stall the
tick. A client that cannot drain its queue is disconnected rather than having
packets dropped. `NetworkChannels.outgoing` is only a fallback for client
entities with no direct sender (tests use it as their packet sink); prefer
`Players`.

## System sets

`voidmc::VoidSystems` names every framework phase so user systems can be
ordered against them with `.before(..)` / `.after(..)`. Ordering only applies
inside one schedule.

| Set | Schedule | What runs there |
|---|---|---|
| `NetworkIngest` | `PreUpdate` | Drain incoming channel, spawn client entities, decode + dispatch (`On<PacketEvent<T>>` observers), handle disconnects. |
| `CommandDrain` | `Update` | Execute queued commands (`CommandSystems::DrainQueue` is inside it). |
| `ItemUseDrain` | `Update` | Run `ItemBehavior` handlers for queued uses/breaks. |
| `MenuClickDrain` | `Update` | Fire `MenuClickEvent` and menu `on_click` handlers (see [Menus](../gameplay/menus.md)). |
| `KeepAlive` | `Update` | Send `KeepAlive`. After `CommandDrain`. |
| `EntitySimulation` | `Update` | Settle spawns, wander, physics. After `KeepAlive`. |
| `ItemPickup` | `Update` | Pickup-delay ticking and item pickup. |
| `EntityBroadcast` | `PostUpdate` | Spawn / movement / motion / metadata packets for non-player entities. |
| `PlayerBroadcast` | `PostUpdate` | Other players' movement and head rotation. |
| `BlockEntitySync` | `PostUpdate` | Block Entity Data packets for changed block entities; before `ChunkStreaming` (see [Block Entities](../gameplay/block-entities.md)). |
| `ChunkStreaming` | `PostUpdate` | Chunk load/unload packets; updates `LoadedChunks`. |
| `InventorySync` | `PostUpdate` | Container packets for changed inventories and open menus (slot, cursor, held slot). |
| `BossBarSync` | `PostUpdate` | Boss bar add / update / remove packets (see [Boss Bars](../gameplay/boss-bars.md)). |
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
