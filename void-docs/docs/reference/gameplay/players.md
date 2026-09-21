# Player Management

## Player Join Flow

When a client completes configuration, the server transitions them to the Play state through these steps:

### 1. Component Insertion (Configuration handler)

Upon receiving `FinishConfigurationAcknowledged`:

- Allocate a `MinecraftEntityId` (`MinecraftEntityId::allocate()`, a process-wide counter shared with spawned entities)
- Compute spawn Y from `WorldGenerator::surface_height_at()` if not explicitly set
- Insert all gameplay components:
  - `ConnectionState(Play)`
  - `MinecraftEntityId`
  - `Position`, `PreviousPosition`, `Rotation`
  - `TeleportState { next_id: 1, pending_id: Some(0) }`
  - `KeepAliveState { last_sent_id: 0, awaiting_response: false }`
  - `CurrentChunkPos`, `EffectiveViewDistance`, `LoadedChunks`
  - `PlayerDimension(Overworld)`

### 2. Initial Packets

The server sends a burst of packets to set up the client:

1. **Play Login** — Game settings (entity ID, dimensions, game mode, view distance, etc.)
2. **Commands** — Protocol command tree for tab-completion
3. **GameEvent(StartWaitingForLevelChunks)** — Tells the client to show the loading screen
4. **SetCenterChunk** — Sets the client's chunk loading center
5. **ChunkDataAndLight** (x N) — Initial chunks around spawn (`initial_chunk_radius`)
6. **SynchronizePlayerPosition** — Teleport the player to spawn coordinates

An optimization sends the teleport after 9 chunks (3x3 center) are sent, so the player can start rendering while remaining chunks arrive.

### 3. PlayerJoinEvent

After all initial packets are sent, `PlayerJoinEvent` is triggered. At this point the player's entity has all components but **does not** have `PlayerReady` yet.

### 4. Player Loaded -> Ready

When the client finishes loading chunks and sends `PlayerLoaded`:

1. The `PlayerReady` marker component is inserted
2. `PlayerReadyEvent` is triggered
3. The observer broadcasts the new player to all other ready players

## Player Ready State

The `PlayerReady` marker component is the key indicator that a player is fully in-game. Most systems filter on it:

```rust
fn my_system(players: Query<&PlayerName, With<PlayerReady>>) {
    for name in players.iter() {
        // Only iterates over fully loaded players
    }
}
```

A player without `PlayerReady` is in a transitional state (loading chunks, not yet visible to others).

## Player Visibility

When a player becomes ready, the `on_player_ready` observer handles mutual visibility:

### For each existing ready player:

1. **PlayerInfoUpdate** sent to the new player (adds existing player to tab list)
2. **SpawnEntity** sent to the new player (creates existing player's entity)
3. **PlayerInfoUpdate** sent to the existing player (adds new player to tab list)
4. **SpawnEntity** sent to the existing player (creates new player's entity)

### Tab List

All player visibility goes through `PlayerInfoUpdate` packets containing:
- UUID
- Player name
- Game mode
- Listed flag (always `true`)

## Player Quit Flow

When a client disconnects:

1. The network thread detects the TCP connection closed
2. Client ID is sent through the `disconnect` channel
3. `ingest_network_packets` drains the disconnect channel
4. The entity is removed from `ClientToEntityMap` and its sender from `ClientSenders`
5. If the player was ready (`PlayerReady` present):
   - `PlayerQuitEvent` is triggered
   - The `on_player_quit` observer broadcasts to all remaining ready players:
     - `RemoveEntities` — Removes the entity from the world
     - `PlayerInfoRemove` — Removes the player from the tab list
6. The entity is despawned

## Position & Rotation Broadcasting

The `broadcast_position` system runs in `PostUpdate` and sends movement updates to all other players:

### Delta Encoding

Position changes are encoded as fixed-point deltas (1/4096 of a block) using the
shared `relative_delta` helper from `systems/entities.rs`:

```rust
let delta_x = relative_delta(pos.x, prev_pos.x); // Option<i16>
let delta_y = relative_delta(pos.y, prev_pos.y);
let delta_z = relative_delta(pos.z, prev_pos.z);
```

An `i16` delta only covers about ±8 blocks. If any axis moved further than that
in a single tick, `relative_delta` returns `None` and the update is sent as an
absolute `TeleportEntity` instead of a saturated delta.

### Rotation Encoding

Yaw and pitch are converted from degrees to a single byte:

```rust
let yaw = (rotation.yaw / 360.0 * 256.0) as u8;
let pitch = (rotation.pitch / 360.0 * 256.0) as u8;
```

### Packets Sent

For each player with changed `Position` or `Rotation`:
1. `UpdateEntityPositionAndRotation` — Combined position delta + rotation, or
   `TeleportEntity` (absolute position, zero velocity) when the move exceeds the
   ~8 block delta range
2. `SetHeadRotation` — Head yaw (for smooth head turning)

The `update_previous_positions` system runs after broadcasting to sync `PreviousPosition` with current `Position`.

## Teleportation

### `Teleport` (with loading barrier)

Insert a `Teleport` on a player and the framework runs the whole handshake:

```rust
commands.entity(player).insert(
    Teleport::to(120.0, 70.0, -40.0)
        .facing(90.0, 0.0)
        .in_dimension(DimensionId::Nether),
);
```

While the component is present:

1. `ServerControlledPosition` is inserted (client movement packets no longer
   update `Position`) and `Position` jumps to the destination so chunk
   streaming starts there immediately. A dimension change updates
   `PlayerDimension`, unloads every chunk and restreams.
2. A `ChunkSendBudget` (default `Teleport::DEFAULT_CHUNK_BUDGET` = 2 per tick)
   throttles the destination chunks; `.chunk_budget(n)` / `.unthrottled()`
   override it.
3. Once every chunk within `preload_radius` (default 2) of the destination has
   been sent, a play `Ping` fences the stream; the matching `Pong` proves the
   client has processed them.
4. `SynchronizePlayerPosition` is sent with a fresh `TeleportState` id.
5. `ConfirmTeleportation` clears the id; the `Teleport` is removed, control and
   the previous budget are restored and `PlayerTeleportEvent { outcome:
   Confirmed }` fires.

A client that never answers is released after `timeout_ticks` (default 600 =
30 s) with `TeleportOutcome::TimedOut`; the position sync is still sent so it
lands at the destination whenever it catches up. Removing the component
yourself yields `TeleportOutcome::Cancelled`. If the player already carried
`ServerControlledPosition` before the teleport (a vehicle seat, say) it is kept.

The barrier advances in `Update` (`VoidSystems::TeleportBarrier`, after
`CommandDrain`).

### `ServerControlledPosition`

A marker: while present the server owns the player's position. `SetPlayerPos`
is ignored and `SetPlayerPosAndRot` updates `Rotation` only (the
`PlayerRotateEvent` still fires, `PlayerMoveEvent` does not). Whoever inserts
it must keep the client in sync — `Teleport` does this for you.

### Raw handshake

The `/tp` command shows the underlying steps:

1. **Update `TeleportState`**: Increment `next_id`, set `pending_id` to the new ID
2. **Update `Position`**: Set the entity's position to target coordinates
3. **Send `SynchronizePlayerPosition`**: Packet with teleport ID, target coordinates, and current rotation
4. **Client confirms**: `ConfirmTeleportation` packet clears `pending_id`

While `pending_id` is `Some`, the server knows a teleportation is in-flight and the client has not yet acknowledged it.

## Abilities and Flight

`PlayerAbilities` is an opt-in component; insert or mutate it and the client
receives the matching Player Abilities packet in `PostUpdate`
(`VoidSystems::AbilitiesSync`):

```rust
commands
    .entity(player)
    .insert(PlayerAbilities::new().flying(true).flying_speed(0.1));
```

| Builder | Effect |
|---|---|
| `allow_flight(bool)` | Player may toggle flight; `false` also stops flying |
| `flying(bool)` | Start/stop flying; `true` also allows flight |
| `invulnerable(bool)`, `instant_build(bool)` | The remaining protocol flags |
| `flying_speed(f32)`, `walking_speed(f32)` | Defaults `0.05` and `0.1` |

When the client toggles flight itself, `flying` is updated in place without a
round trip. If flight is not allowed the component is left with
`flying = false` and re-sent, which puts the client back on the ground.
`PlayerToggleFlyEvent` fires either way.

## Chat Messages

When a player sends a chat message:

1. The server formats it as `<PlayerName> message`
2. A `SystemChat` packet (NBT text component) is broadcast to all ready players
3. `ChatMessageEvent` is triggered for plugin observers

Messages starting with `/` are intercepted and routed to the command system, even if sent as `ChatMessage` rather than `ChatCommand` (this handles commands the client doesn't recognize from the command tree).
