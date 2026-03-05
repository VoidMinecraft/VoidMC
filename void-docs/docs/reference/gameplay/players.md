# Player Management

## Player Join Flow

When a client completes configuration, the server transitions them to the Play state through these steps:

### 1. Component Insertion (Configuration handler)

Upon receiving `FinishConfigurationAcknowledged`:

- Allocate a `MinecraftEntityId` from `EntityIdCounter`
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
4. The entity is removed from `ClientToEntityMap`
5. If the player was ready (`PlayerReady` present):
   - `PlayerQuitEvent` is triggered
   - The `on_player_quit` observer broadcasts to all remaining ready players:
     - `RemoveEntities` — Removes the entity from the world
     - `PlayerInfoRemove` — Removes the player from the tab list
6. The entity is despawned

## Position & Rotation Broadcasting

The `broadcast_position` system runs in `PostUpdate` and sends movement updates to all other players:

### Delta Encoding

Position changes are encoded as fixed-point deltas:

```rust
let delta_x = ((pos.x * 32.0 - prev_pos.x * 32.0) * 128.0) as i16;
let delta_y = ((pos.y * 32.0 - prev_pos.y * 32.0) * 128.0) as i16;
let delta_z = ((pos.z * 32.0 - prev_pos.z * 32.0) * 128.0) as i16;
```

### Rotation Encoding

Yaw and pitch are converted from degrees to a single byte:

```rust
let yaw = (rotation.yaw / 360.0 * 256.0) as u8;
let pitch = (rotation.pitch / 360.0 * 256.0) as u8;
```

### Packets Sent

For each player with changed `Position` or `Rotation`:
1. `UpdateEntityPositionAndRotation` — Combined position delta + rotation
2. `SetHeadRotation` — Head yaw (for smooth head turning)

The `update_previous_positions` system runs after broadcasting to sync `PreviousPosition` with current `Position`.

## Teleportation

Server-initiated teleportation (e.g., `/tp` command) works through:

1. **Update `TeleportState`**: Increment `next_id`, set `pending_id` to the new ID
2. **Update `Position`**: Set the entity's position to target coordinates
3. **Send `SynchronizePlayerPosition`**: Packet with teleport ID, target coordinates, and current rotation
4. **Client confirms**: `ConfirmTeleportation` packet clears `pending_id`

While `pending_id` is `Some`, the server knows a teleportation is in-flight and the client has not yet acknowledged it.

## Chat Messages

When a player sends a chat message:

1. The server formats it as `<PlayerName> message`
2. A `SystemChat` packet (NBT text component) is broadcast to all ready players
3. `ChatMessageEvent` is triggered for plugin observers

Messages starting with `/` are intercepted and routed to the command system, even if sent as `ChatMessage` rather than `ChatCommand` (this handles commands the client doesn't recognize from the command tree).
