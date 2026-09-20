# Biomes

Biomes live in chunk sections at the protocol's 4×4×4 cell granularity. Void
ships the 65 vanilla biomes, resolves every id through the synced registry, and
lets a server read and write cells, vary biomes in its generator, and register
custom biomes before players log in.

## Ids

`BiomeId` is the index of a biome in the `minecraft:worldgen/biome` registry
sent at login. Never write a literal: resolve names.

```rust
use voidmc::{BiomeId, RegistryDataStore};

let desert = BiomeId::named("minecraft:desert").unwrap();   // vanilla, no world needed
let plains = BiomeId::plains();
let custom = world.resource::<RegistryDataStore>().biome("myserver:crimson_sky"); // vanilla or custom
```

## Reading and writing cells

```rust
use voidmc::{BlockPosition, DimensionId, biome_at, set_biome};

let pos = BlockPosition { x: 17, y: 70, z: -3 };
let current = biome_at(world, DimensionId::Overworld, pos);            // Option<BiomeId>
let previous = set_biome(world, DimensionId::Overworld, pos, desert);  // Option<BiomeId>
```

`set_biome` changes the whole 4×4×4 cell containing the position, marks the
chunk `ChunkDirty` (so `void-world-io` persists it) and sends a `ChunksBiomes`
packet to every player whose `LoadedChunks` contains the chunk — no chunk
resend. Both return `None` when the chunk is not loaded or `y` is out of range.
Section palettes are re-encoded on write: single value, indirect at 1–3 bits, or
direct at `ceil(log2(registry size))` bits, the only widths the client accepts.

## Generation

`WorldGenerator::cell_biome(x, y, z)` returns the biome of the cell whose lowest
corner is at those block coordinates; the default is plains, so existing
generators are unchanged. Feed it to `ChunkBuilder::biomes_from` in
`generate_chunk`:

```rust
impl WorldGenerator for Islands {
    fn generate_chunk(&self, pos: &ChunkPos) -> Chunk {
        ChunkBuilder::new(pos.x, pos.z)
            .biomes_from(|x, y, z| self.cell_biome(x, y, z).0)
            .fill_below(64, blocks::STONE)
            .build()
    }

    fn cell_biome(&self, x: i32, _y: i32, z: i32) -> BiomeId {
        if (x * x + z * z) < 64 * 64 { BiomeId::named("beach").unwrap() } else { BiomeId::named("ocean").unwrap() }
    }
}
```

A generator that places more than eight biomes in one section and runs with
custom biomes registered should call `ChunkBuilder::biome_registry_size(n)`;
the framework also re-widens direct palettes to the synced registry size when a
chunk is loaded or generated.

## Custom biomes

`BiomeBuilder` produces the network form of a biome (climate, `effects`,
syncable `attributes`) and registers it before the Configuration phase sends
registries. Colours are `0xRRGGBB`; attributes are the 26.1 environment
attributes (fog, sky, music, ambient sounds and particles), either a value or a
modifier over the dimension's value.

```rust
use voidmc::{Attribute, BiomeBuilder, ServerConfigBuilder, Tag};

ServerConfigBuilder::new().configure_registries(|registries| {
    let id = BiomeBuilder::new("myserver:crimson_sky")
        .precipitation(false)
        .temperature(1.2)
        .sky_color(0xff0044)
        .fog_color(0x220011)
        .water_color(0x5f1030)
        .music("minecraft:music.nether.basalt_deltas", 12000, 24000)
        .attribute("minecraft:visual/water_fog_end_distance", Attribute::modified("multiply", Tag::Float(0.85)))
        .register(registries)
        .unwrap();
});
```

`register` returns the new `BiomeId` and fails on an unqualified name, a
duplicate, or an attribute the client does not sync (gameplay attributes are
server-only). It must run before any client logs in (a debug assertion checks
`RegistryDataStore::sent_to_clients`): ids are positions in the synced list, so
a late addition would not match what earlier clients received. `Tag` is
`voidmc::Tag`, the NBT tag type used for raw attribute values. Later, `RegistryDataStore::biome("myserver:crimson_sky")` finds it
again. Visual and audio attributes apply at the camera's biome only; a custom
biome must therefore surround the player to be seen.

The example server registers `void_example:crimson_sky` and offers
`/biome [name]` to show the current biome or set the cell you stand in
(`void-example/src/biome.rs`).
