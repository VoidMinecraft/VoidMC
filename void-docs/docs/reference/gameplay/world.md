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

1. **Check movement**: Skip if the player hasn't moved to a new chunk column and view distance hasn't changed.
2. **Cap view distance**: `min(client_view_distance, server_view_distance)` — stored in `EffectiveViewDistance`.
3. **Send SetCenterChunk**: When the player's chunk position changes, notify the client.
4. **Unload out-of-range**: Send `UnloadChunk` for chunks no longer within view distance, remove from `LoadedChunks`.
5. **Load in-range**: For chunks within view distance not yet loaded:
   - If the chunk entity exists in `ChunkIndex`, send its data.
   - If not, generate it on-demand using the `WorldGenerator`, spawn a chunk entity, and send the data.
   - New chunks are sent in nearest-first order (from `chunks_in_radius` sorting).

## Spawn Area Pre-Generation

During `Startup`, the `init_world` system pre-generates chunks around the spawn point:

```rust
fn init_world(commands, chunk_index, world_gen, config) {
    let spawn_chunk = ChunkPos::from_block(config.spawn_x, config.spawn_z);
    for pos in spawn_chunk.chunks_in_radius(config.spawn_chunk_radius) {
        // Generate chunk, spawn entity, add to ChunkIndex
    }
}
```

This ensures the spawn area is ready before any player connects. The radius is controlled by `ServerConfig::spawn_chunk_radius` (default: 10).

## WorldGenerator Trait

```rust
pub trait WorldGenerator: Send + Sync {
    /// Generate a full chunk at the given position.
    fn generate_chunk(&self, pos: &ChunkPos) -> ProtocolChunk;

    /// Return the terrain surface Y at a given block coordinate.
    fn surface_height_at(&self, block_x: i32, block_z: i32) -> i32;
}
```

`surface_height_at` is used by the server to compute the spawn Y coordinate when `SpawnPosition::y` is `None`.

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

Implement `WorldGenerator` and pass it to the builder:

```rust
use voidmc::{DefaultWorldGenerator, ServerBuilder, WorldGenerator};
use voidmc::world::ChunkPos;

struct FlatWorldGenerator;

impl WorldGenerator for FlatWorldGenerator {
    fn generate_chunk(&self, pos: &ChunkPos) -> voidmc_protocol::clientbound::chunk::Chunk {
        voidmc_protocol::clientbound::chunk::ChunkBuilder::new(pos.x, pos.z)
            .with_flat_layer(64, voidmc_protocol::clientbound::chunk::blocks::GRASS_BLOCK)
            .build()
    }

    fn surface_height_at(&self, _block_x: i32, _block_z: i32) -> i32 {
        64
    }
}

let config = ServerBuilder::new()
    .world_generator(FlatWorldGenerator)
    .build();
```
