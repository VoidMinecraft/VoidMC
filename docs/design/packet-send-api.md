# Packet send API — design note

_2026-09-20 · branch `feat/packet-send-api`_

## Problem

Every clientbound send in `void` is the literal
`let _ = channels.outgoing.send(OutgoingPacket { client_id, packet })` (36 sites).
Recipients are `u32` client ids, so callers query `&ClientId` first; errors are
dropped; every broadcast is a hand-written `With<PlayerReady>` loop with a
`packet.clone()` per recipient; the only helpers are file-private or `pub(crate)`.
Upcoming features (boss bar, sounds, particles, containers) are all "send packet
X to a set of players" and would each re-implement this.

## Decision

A `players` module in `void` exposing two entry points with one shared API:

- `Players` — a `SystemParam` for systems and observers.
- `WorldPlayers::new(&World)` — for exclusive-world code (command handlers,
  item behaviours, drains). Uses `World::try_query_filtered` so it works from
  `&World` (no `&mut` borrow dance, no `outgoing.clone()`).

Both address players by `Entity`. Packets are accepted as
`impl Into<ClientboundPacket>`; `voidmc-protocol` gains `From<T>` impls for every
packet struct (one macro table in `clientbound.rs`), so callers write
`players.send(entity, KeepAlive { .. })`.

```rust
players.send(entity, packet);                  // one client, ready or not
players.broadcast(packet);                     // every ready player
players.ready().except(me).send(packet);       // exclusion
players.ready().in_dimension(dim).send(packet);
players.ready().seeing_chunk(dim, pos).send(packet);  // uses LoadedChunks
players.ready().filter(|r| r.entity() != me).send(packet);
```

`ready()` returns a `Recipients` value: a snapshot (`Vec<Recipient>`) that
composes filters and ends in `.send()`. Filters are plain methods, not a trait,
so nothing needs importing.

Failure handling, logged exactly once per failure and never at two layers:

| Situation | Level | Rationale |
|---|---|---|
| entity no longer exists | `debug` | normal race (player left this tick) |
| entity exists, no `ClientId` | `warn` | developer error (not a client entity) |
| outgoing channel closed | `error`, once per process | network thread is gone; per-packet spam is useless |

## Alternatives considered

- **Keep `u32` client ids, add helpers only.** Cheapest, but keeps every caller
  querying `&ClientId` and stays incompatible with an entity-centric ECS API.
- **A shared trait (`PacketSender`) implemented by both entry points.**
  Removes ~40 lines of delegation but forces users to import the trait.
  Rejected for ergonomics; the duplication is thin method shells.
- **Lazy iterator-typed `Recipients<I>`.** Avoids one `Vec` allocation per
  broadcast. Packet clones and channel sends dominate; not worth generic
  types leaking into user code.
- **Requiring `&mut World` for the exclusive variant.** Would allow
  `query_filtered`, but `CommandContext::reply`, `send_ack` and the item
  contexts only hold `&World`. `try_query_filtered` + per-entity `world.get`
  for the optional components (dimension, loaded chunks) avoids any
  component-registration prerequisite.
- **Commands-based deferred sending** (`commands.queue(...)`). Would change
  ordering relative to the current immediate send; rejected to keep behaviour.

## Scheduling surface

Public `SystemSet`s (`VoidSystems`) wrap the framework phases so user systems
can be ordered against them: `NetworkIngest` (PreUpdate), `Tick` (Update:
keep-alive, settle, wander, physics), `EntityBroadcast`, `PlayerBroadcast`,
`ChunkStreaming`, `InventorySync` (PostUpdate), plus the existing
`CommandSystems::DrainQueue`. Names follow what the systems do today.

## Non-goals

Entity view-distance culling, per-packet batching, and async back-pressure are
unchanged; `NetworkChannels` and `OutgoingPacket` stay public as the raw layer.
