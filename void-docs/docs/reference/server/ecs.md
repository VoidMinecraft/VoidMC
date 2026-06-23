# ECS Components & Resources

Void uses [Bevy ECS](https://bevyengine.org/) to represent all server state as entities with components, and shared state as resources.

## Components

### Connection

| Component | Fields | Description |
|---|---|---|
| `Client` | (marker) | Marker component present on all client entities |
| `ClientId(u32)` | Internal network ID | Unique identifier assigned by the network layer |
| `ConnectionState(State)` | Protocol state enum | Current protocol state (`Handshake`, `Status`, `Login`, `Configuration`, `Play`) |
| `ProtocolVersion(i32)` | Version number | Client's declared protocol version from handshake |

### Player Identity

| Component | Fields | Description |
|---|---|---|
| `PlayerName(String)` | Username | Player's Minecraft username (set during Login) |
| `PlayerUuid(Uuid)` | UUID | Player's UUID (set during Login) |
| `MinecraftEntityId(i32)` | Entity ID | Server-assigned Minecraft entity ID (visible to all clients) |
| `Operator` | (marker) | Marks a player as an operator/admin |

### Player State

| Component | Fields | Description |
|---|---|---|
| `Position { x, y, z }` | `f64` coords | Current world position |
| `PreviousPosition { x, y, z }` | `f64` coords | Position from the previous tick (used for delta encoding) |
| `Rotation { yaw, pitch }` | `f32` angles | Current look direction |
| `PlayerReady` | (marker) | Added when the client sends `PlayerLoaded` — indicates the player is fully in-game |
| `PlayerDimension(DimensionId)` | Dimension | Which dimension the player is currently in |
| `ClientSettings { locale, view_distance }` | Settings | Client preferences received during configuration/play |

### Teleportation

| Component | Fields | Description |
|---|---|---|
| `TeleportState { next_id, pending_id }` | `i32`, `Option<i32>` | Tracks teleport confirmations — `pending_id` is cleared when the client confirms |

### Keep-Alive

| Component | Fields | Description |
|---|---|---|
| `KeepAliveState { last_sent_id, awaiting_response }` | `i64`, `bool` | Tracks the last keep-alive ID sent and whether a response is pending |

### Chunk Streaming

| Component | Fields | Description |
|---|---|---|
| `CurrentChunkPos(ChunkPos)` | Chunk column | The chunk the player is currently standing in |
| `EffectiveViewDistance(i32)` | Distance | The capped view distance used for chunk streaming |
| `LoadedChunks(HashSet<ChunkPos>)` | Loaded set | Chunks currently sent to this player |

### Non-Player Entities

Summoned entities are ordinary ECS entities without `ClientId` or player
identity components. They are authoritative on the game thread and are synced
to ready players by `systems::entities`.

| Component | Fields | Description |
|---|---|---|
| `MinecraftEntityId(i32)` | Entity ID | Server-assigned ID used by entity packets |
| `EntityUuid(Uuid)` | UUID | UUID sent once in `Add Entity` |
| `EntityType(i32)` | Registry ID | Protocol ID from `minecraft:entity_type` |
| `SpawnedEntity` | (marker) | Marks a non-player entity managed by the entity lifecycle systems |
| `EntityDimension(DimensionId)` | Dimension | Dimension visibility filter for player recipients |
| `Position { x, y, z }` | `f64` coords | Current world position |
| `PreviousPosition { x, y, z }` | `f64` coords | Last synced position, used for relative movement packets |
| `Rotation { yaw, pitch }` | `f32` angles | Current body/look rotation |
| `Velocity { x, y, z }` | `f64` vector | Velocity encoded directly as protocol LP Vec3 |

### Chunk Entity Components

Chunks are also ECS entities with these components:

| Component | Fields | Description |
|---|---|---|
| `ChunkPosition(ChunkPos)` | `{ x, z }` | The chunk's column position |
| `ChunkData` | `sections`, `heightmaps`, `light` | Block data, heightmaps, and lighting |
| `ChunkDimension(DimensionId)` | Dimension | Which dimension this chunk belongs to |

## Resources

| Resource | Description |
|---|---|
| `ServerConfigResource` | Runtime-readable server configuration (see [Configuration](/reference/server/configuration)) |
| `WorldGen(Box<dyn WorldGenerator>)` | Active world generator |
| `RegistryDataStore` | Minecraft registry data (see [Registry](/reference/gameplay/registry)) |
| `EntityIdCounter(i32)` | Auto-incrementing counter for Minecraft entity IDs |
| `ChunkIndex(HashMap<(DimensionId, ChunkPos), Entity>)` | Spatial index for O(1) chunk entity lookup |
| `NetworkChannels` | Flume channel senders/receivers for network communication |
| `ClientToEntityMap(HashMap<u32, Entity>)` | Maps network client IDs to ECS entities |
| `CommandRegistry` | Registered commands (see [Commands](/reference/gameplay/commands)) |
| `KeepAliveTicker` | Tick counter for keep-alive scheduling (default: 200 tick interval) |

## Entity Lifecycle

### Client Entity Creation

When the first packet arrives from a new client, `ingest_network_packets` spawns an entity with:
- `Client` (marker)
- `ClientId(id)`
- `ConnectionState(Handshake)`

### Component Insertion During Connection

As the client progresses through protocol states, handlers insert additional components:

1. **Handshake**: `ProtocolVersion`, updated `ConnectionState`
2. **Login**: `PlayerName`, `PlayerUuid`
3. **Configuration**: `ClientSettings`
4. **Finish Configuration** (transition to Play): `MinecraftEntityId`, `Position`, `PreviousPosition`, `Rotation`, `TeleportState`, `KeepAliveState`, `CurrentChunkPos`, `EffectiveViewDistance`, `LoadedChunks`, `PlayerDimension`
5. **Play (PlayerLoaded)**: `PlayerReady` marker

### Entity Despawn

When a client disconnects:
1. The network thread sends the client ID through the `disconnect` channel
2. `ingest_network_packets` removes the client from `ClientToEntityMap`
3. If the player was ready (`PlayerReady` present), a `PlayerQuitEvent` is triggered
4. The entity is despawned with `world.despawn(entity)`

### Non-Player Entity Lifecycle

Non-player entities use the same `MinecraftEntityId`, `Position`,
`PreviousPosition`, and `Rotation` components as players, plus the dedicated
components listed above.

1. Spawn an entity with `SpawnedEntity`, `EntityType`, `EntityUuid`,
   `EntityDimension`, `Velocity`, and position/rotation components. The
   default `/summon` command does this after validating the entity type through
   `voidmc-data`.
2. During `PostUpdate`, `broadcast_entity_spawns` detects newly added
   `SpawnedEntity` entities and sends `Add Entity` to ready players in the same
   dimension.
3. When a player becomes ready, `on_player_ready_spawn_entities` replays all
   currently visible spawned entities to that player.
4. Position or rotation changes are broadcast by `broadcast_entity_movement`.
   Small movements use relative move packets; moves outside the ±8 block delta
   budget use `Teleport Entity`. Rotation changes also send `Rotate Head`.
5. Velocity changes are broadcast by `broadcast_entity_motion` using `Set Entity
   Motion`. LP Vec3 values are used directly; the old `velocity * 8000` short
   encoding is not used by protocol 26.1.2.
6. Trigger `EntityDespawnEvent { entity }` to remove a spawned entity through
   the lifecycle system. Observers send `Remove Entities` to visible players and
   then despawn the ECS entity.

This lifecycle currently handles visibility, spawn/replay, motion, movement,
and removal. It does not yet implement AI, metadata (`Set Entity Data`),
equipment, passengers, or per-entity view-distance culling.
