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
