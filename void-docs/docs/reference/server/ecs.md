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
| `SpawnedEntity` | (marker) | Marks a non-player entity; constructible only inside `void`, so `EntityBuilder` is the one spawn path and every component below plus `EntityViewers` / `EntityMetadata` is guaranteed present. Use it as a query filter |
| `EntityDimension(DimensionId)` | Dimension | Dimension the entity lives in (default `Overworld`) |
| `EntityViewers` | Player set | Players currently receiving this entity's packets; maintained by the visibility tracker |
| `EntityMetadata` | Indexed values | Synched entity data with dirty tracking; typed components (`CustomName`, `Glowing`, `Display`, ...) project into it |
| `Passengers(Vec<Entity>)` | Riders | Entities riding this one; changes send `SetPassengers` to viewers |
| `Mount(Entity)` | Vehicle | Read-only back-reference on each passenger, mirrored from `Passengers`; removed on dismount or vehicle despawn |
| `Position { x, y, z }` | `f64` coords | Current world position |
| `PreviousPosition { x, y, z }` | `f64` coords | Last synced position, used for relative movement packets |
| `Rotation { yaw, pitch }` | `f32` angles | Current body/look rotation |
| `Velocity { x, y, z }` | `f64` vector | Velocity encoded directly as protocol LP Vec3 |
| `EntityCollider { half_width, height, step_height }` | `f64` dimensions | Feet-anchored collision box used by server-side movement |
| `MovementConfig` | Feature flags | Enables wandering, gravity, and block collision checks |
| `VerticalVelocity(f64)` | Vertical speed | Gravity state kept separately from protocol velocity |
| `Grounded(bool)` | Contact state | Whether physics found support directly below the collision box |
| `Wander` | Direction state | Optional demo random-walk behavior attached by `/summon --wander` |

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
| `ChunkIndex(HashMap<(DimensionId, ChunkPos), Entity>)` | Spatial index for O(1) chunk entity lookup |
| `NetworkChannels` | Flume channel senders/receivers for network communication |
| `ClientSenders` | Per-client bounded outbound senders, keyed by client entity |
| `ClientToEntityMap(HashMap<u32, Entity>)` | Maps network client IDs to ECS entities |
| `CommandRegistry` | Registered commands (see [Commands](/reference/gameplay/commands)) |
| `KeepAliveTicker` | Tick counter for keep-alive scheduling (default: 200 tick interval) |

## Entity Lifecycle

### Client Entity Creation

When the first packet arrives from a new client, `ingest_network_packets` spawns an entity with:
- `Client` (marker)
- `ClientId(id)`
- `ConnectionState(Handshake)`

and moves the client's outbound sender (announced by the network thread on
accept) into `ClientSenders` under that entity, so every packet sent to the
entity goes straight to its own queue.

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
2. `ingest_network_packets` removes the client from `ClientToEntityMap` and drops its sender from `ClientSenders`
3. If the player was ready (`PlayerReady` present), a `PlayerQuitEvent` is triggered
4. The entity is despawned with `world.despawn(entity)`

A client whose outbound queue overflows is kicked by the network thread and
then follows exactly this path.

### Non-Player Entity Lifecycle

Non-player entities use the same `MinecraftEntityId`, `Position`,
`PreviousPosition`, and `Rotation` components as players, plus the dedicated
components listed above.

1. Spawn with `EntityBuilder`:
   `EntityBuilder::new(EntityKind::Zombie).at(x, y, z).in_dimension(dim).gravity(true).spawn(&mut commands)`
   (or `.spawn_in(&mut world)`). It allocates the network id and UUID and
   fills every component `SpawnedEntity` requires; extra components are
   inserted on the returned `EntityCommands`. `EntityKind` is generated from
   `minecraft:entity_type` (`EntityKind::from_name`, `.id()`, `.name()`).
   `.settle_ticks(n)` bounds the ground-snap window of `settle_recent_spawns`
   (default 15).
2. `PostUpdate` (`VoidSystems::EntityVisibility`, after chunk streaming): the
   tracker resolves each entity's viewers from a chunk-indexed lookup of the
   ready players whose `LoadedChunks` contain the entity's chunk in its
   dimension, rather than scanning every ready player. New viewers get
   `Add Entity` (and an `EntityShownEvent`), viewers that stopped seeing the
   chunk get `Remove Entities` (and an `EntityHiddenEvent`). Late joiners are
   covered the same way once their chunks load; disconnected players simply
   drop out of the set. The diff runs only for entities whose chunk or
   dimension changed, or that stand in a chunk whose viewer set changed this
   tick (a player loading or unloading it, changing dimension, joining or
   leaving).
3. Position or rotation changes are broadcast by `broadcast_entity_movement`
   to current viewers only (`VoidSystems::EntityBroadcast`, before the
   tracker, so a newly shown viewer never receives a delta on top of the spawn
   position). Small movements use relative move packets; moves outside the ±8
   block delta budget use `Teleport Entity`. Rotation changes also send
   `Rotate Head`.
4. Velocity changes are broadcast by `broadcast_entity_motion` using `Set Entity
   Motion` to current viewers.
5. Remove with `commands.entity(e).despawn()` (or `world.despawn(e)`). An
   `On<Remove, SpawnedEntity>` observer sends `Remove Entities` to every
   current viewer, so a client can never keep a ghost. `EntityDespawnEvent`
   remains as a trigger-style hook that does the same despawn.

Features that need extra packets per viewer observe
`EntityShownEvent { entity, viewer }` instead of broadcasting on spawn; entity
metadata and passengers ship this way (see
[Entities](/reference/gameplay/entities)). Not yet implemented: mob AI,
equipment.
