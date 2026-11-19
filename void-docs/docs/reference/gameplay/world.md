# World & Chunk System

## ChunkPos

`ChunkPos` represents a chunk column position in the world:

```rust
pub struct ChunkPos {
    pub x: i32,
    pub z: i32,
}
```

### Methods

| Method | Description |
|---|---|
| `ChunkPos::new(x, z)` | Create from chunk coordinates |
| `ChunkPos::from_block(block_x, block_z)` | Convert block coordinates (`f64`) to chunk position (`>> 4`) |
| `chebyshev_distance(&self, other)` | Chebyshev (chessboard) distance between two chunk positions |
| `chunks_in_radius(&self, radius)` | All chunk positions within a Chebyshev radius, sorted by squared Euclidean distance (nearest first) |

## DimensionId

```rust
pub enum DimensionId {
    Overworld,  // protocol_id: 0, name: "minecraft:overworld"
    Nether,     // protocol_id: 1, name: "minecraft:the_nether"
    End,        // protocol_id: 2, name: "minecraft:the_end"
}
```

## Chunk Entity Components

Chunks are stored as ECS entities with three components:

| Component | Description |
|---|---|
| `ChunkPosition(ChunkPos)` | The chunk's column position |
| `ChunkData { sections, heightmaps, light }` | Block data, heightmaps, and lighting information |
| `ChunkDimension(DimensionId)` | Which dimension this chunk belongs to |

`ChunkData` provides:
- `from_protocol_chunk(chunk)` — Create from a protocol `Chunk` object
- `to_packet(x, z)` — Convert to a `ChunkDataAndLight` packet for sending to clients

## ChunkIndex

`ChunkIndex` is a global ECS resource providing O(1) chunk entity lookup:

```rust
pub struct ChunkIndex(pub HashMap<(DimensionId, ChunkPos), Entity>);
```

Use it to find chunk entities by dimension and position:

```rust
fn my_system(chunk_index: Res<ChunkIndex>, chunks: Query<&ChunkData>) {
    let key = (DimensionId::Overworld, ChunkPos::new(0, 0));
    if let Some(&entity) = chunk_index.0.get(&key) {
        let data = chunks.get(entity).unwrap();
        // ...
    }
}
```

## Chunk Streaming

The `stream_chunks` system (runs in `PostUpdate`) manages chunk loading and unloading for each player:

1. **Check movement**: Skip if the player hasn't moved to a new chunk column, view distance hasn't changed and they carry no `ChunkStreamBacklog`.
2. **Cap view distance**: `min(client_view_distance, server_view_distance)` — stored in `EffectiveViewDistance`.
3. **Send SetCenterChunk**: When the player's chunk position changes, notify the client.
4. **Unload out-of-range**: Send `UnloadChunk` for chunks no longer within view distance, remove from `LoadedChunks`.
5. **Load in-range**: For chunks within view distance not yet loaded:
   - If the chunk entity exists in `ChunkIndex`, send its data.
   - If not, generate it on-demand using the `WorldGenerator`, spawn a chunk entity, and send the data.
   - New chunks are sent in nearest-first order (from `chunks_in_radius` sorting, squared Euclidean distance from the player's chunk). Once a chunk has to wait (send budget, generation cap, or spawn still pending), every farther chunk waits behind it, so a cached chunk never overtakes a nearer one that still has to be generated.
   - The chunks sent to a player in one tick are framed by `ChunkBatchStart` and `ChunkBatchFinished`, like vanilla.
6. **Throttle**: A player with a `ChunkSendBudget(n)` receives at most `n` chunk packets per tick (cached ones included). When the budget, the `max_chunk_generations_per_tick` cap or a chunk still pending spawn leaves chunks unsent, the unsent tail is stored nearest-first in `ChunkStreamBacklog` and drained on the following ticks without recomputing the range or the unload set (steps 4–5 only rerun when the chunk column or view distance changes); the component is removed once the range is complete. Players without a budget receive their whole range at once.

The join sends the square of `initial_chunk_radius` around spawn, capped by the same effective view distance, so a player is never sent chunks that the first streaming pass would forget. The `Login` packet carries the server's `view_distance` (the client's chunk-cache radius) and `simulation_distance`.

## Spawn Area Pre-Generation

During `Startup`, the `init_world` system pre-generates chunks around the spawn point:

```rust
fn init_world(commands, chunk_index, world_gen, loader, registries, config) {
    let spawn_chunk = ChunkPos::from_block(config.spawn_x, config.spawn_z);
    for pos in spawn_chunk.chunks_in_radius(config.spawn_chunk_radius) {
        // load_or_generate: the ChunkLoaderResource if any, else the WorldGen resource
        // Spawn the chunk entity, add it to ChunkIndex
    }
}
```

This ensures the spawn area is ready before any player connects. The radius is controlled by `ServerConfig::spawn_chunk_radius` (default: 10). The spawn area, chunks streamed later and the spawn height all come from the `WorldGen` resource, the generator passed to `ServerConfigBuilder::world_generator`.

A joining player is sent the chunks within the join radius (`initial_chunk_radius` capped by the effective view distance, see [Chunk Streaming](#chunk-streaming)) that are already in the spawn area. If that radius reaches past the spawn area, chunk streaming generates the missing chunks on the player's first pass, even when the player's view distance equals the join radius.

### Generation Cost Controls

Three settings decide when chunks are generated, never what they contain:

| Setting | Effect |
|---|---|
| `spawn_chunk_radius` | Chunks generated at startup instead of when a player first needs them |
| `max_chunk_generations_per_tick` | Caps the chunks streaming generates per tick (default 8, `0` = no cap); the rest wait in `ChunkStreamBacklog` |
| `ChunkIndex` | Every chunk is generated once, then served from its entity to every player and on every revisit |

A chunk is the same, byte for byte, whether it was generated at startup, streamed later under the budget, or generated again by a fresh `WorldGenerator`. The end-to-end scenarios in `void-e2e/tests/scenarios/atp_027_world_generation.rs` check this against a terrain fixture for `DefaultWorldGenerator`.

## WorldGenerator Trait

```rust
pub trait WorldGenerator: Send + Sync {
    /// Generate a full chunk at the given position.
    fn generate_chunk(&self, pos: &ChunkPos) -> ProtocolChunk;

    /// Return the Y of the first air block above the terrain and any water at a given block coordinate.
    fn surface_height_at(&self, block_x: i32, block_z: i32) -> i32;

    /// Biome of the 4×4×4 cell whose lowest corner is at these block coordinates (default: plains).
    fn cell_biome(&self, block_x: i32, block_y: i32, block_z: i32) -> BiomeId { BiomeId::plains() }
}
```

`ProtocolChunk` is `voidmc_protocol::clientbound::chunk::Chunk`. `generate_chunk` receives the chunk column position; its block `(x, z)` (0..16) is at world `(pos.x * 16 + x, pos.z * 16 + z)`.

`surface_height_at` returns the Y of the first air block above the terrain and any water (where an entity standing on the surface has its feet). When `SpawnPosition::y` is `None`, the server spawns players at `surface_height_at(spawn_x.floor(), spawn_z.floor())`; an explicit `SpawnPosition::y` is used as is and the generator is not asked.

`cell_biome` is described in [Biomes](./biomes.md#generation).

The free functions `voidmc::world::generation::generate_chunk` and `surface_height_at` always use `DefaultWorldGenerator::default()`, whatever generator the server runs; on a running server, read the `WorldGen` resource instead. `generate_spawn_area` is deprecated for the same reason; the server never calls it.

## DefaultWorldGenerator

The built-in generator produces sine-wave terrain:

```rust
pub struct DefaultWorldGenerator {
    pub base_height: i32,   // default: 64
    pub frequency: f64,     // default: 0.02
    pub amplitude: f64,     // default: 8.0
    pub water_level: i32,   // default: 62
}
```

The terrain height at any block `(x, z)` is computed as:

```
height = base_height + ((sin(x * freq) + sin(z * freq) + detail) * amplitude)
```

Where `detail` adds smaller-scale variation:

```
detail = sin(x * freq * 3.7) * 0.3 + sin(z * freq * 2.9) * 0.3
```

The generator uses `ChunkBuilder` from `void-protocol` with:
- Layered heightmap: grass block (1 layer), dirt (4 layers), stone (rest)
- Water fill at `water_level`

## Custom World Generators

Implement `WorldGenerator` and pass it to the builder. Chunks are built with
`ChunkBuilder` from `voidmc-protocol`, which the application adds as a
dependency next to `voidmc`, at the version `voidmc` uses:

```rust
use voidmc::{ChunkPos, ServerConfigBuilder, WorldGenerator};
use voidmc_protocol::clientbound::chunk::{Chunk, ChunkBuilder, blocks};

struct FlatWorldGenerator;

impl WorldGenerator for FlatWorldGenerator {
    fn generate_chunk(&self, pos: &ChunkPos) -> Chunk {
        ChunkBuilder::new(pos.x, pos.z)
            .with_heightmap(|_, _| 64, blocks::GRASS_BLOCK)
            .build()
    }

    fn surface_height_at(&self, _block_x: i32, _block_z: i32) -> i32 {
        64
    }
}

let config = ServerConfigBuilder::new()
    .world_generator(FlatWorldGenerator)
    .build();
```

`blocks` only names a few states; any other block state id, such as those of
`voidmc_data`, can be placed with `ChunkBuilder::set_block`. A section holding
more than 256 distinct states is sent with global ids, like vanilla.

The end-to-end scenario `void-e2e/tests/scenarios/atp_036_custom_world_generator.rs`
runs this example and a generator with a recognizable pattern, checked block for
block on the client.
