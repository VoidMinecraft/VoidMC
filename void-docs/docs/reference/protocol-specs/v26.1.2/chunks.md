# Chunks & Lighting

A *chunk* is a 16×384×16 column of blocks divided into 24 vertical *sections* of 16×16×16. The server sends a chunk to the client as one `Level Chunk With Light` packet, which is the concatenation of two well-defined sub-payloads: chunk data (heightmaps + block/biome contents + block entities) and light data (sky/block light bitmasks + arrays). Light updates outside of chunk loads are sent independently as `Update Light` and reuse the same light sub-payload.

## Level Chunk With Light packet

| Field | Type | Notes |
|-------|------|-------|
| Chunk X | [Int](./data-types#int) | Chunk coordinate. |
| Chunk Z | [Int](./data-types#int) | Chunk coordinate. |
| Chunk data | sub-payload | See [Chunk data](#chunk-data). |
| Light data | sub-payload | See [Light data](#light-data). |

## Chunk data

| Field | Type | Notes |
|-------|------|-------|
| Heightmaps | [NBT](./data-types#nbt)/map | Map of heightmap type → packed long array. |
| Data length | [VarInt](./data-types#varint) | Length in bytes of the section blob that follows. |
| Data | byte[] | Concatenation of all 24 chunk sections (see [Chunk section](#chunk-section)). |
| Block entity count | [VarInt](./data-types#varint) | Number of block entities to follow. |
| Block entities | Array | Repeats `count` times, see [Block entity entry](#block-entity-entry). |

### Heightmaps

Sent as a map keyed by heightmap type (an enum identifier serialized as VarInt) to a packed long array of height values. Two types are sent to the client:

| Type | Notes |
|------|-------|
| `MOTION_BLOCKING` | Y of the highest block that blocks motion or contains a fluid (used by mob spawning, AI). |
| `WORLD_SURFACE` | Y of the highest non-air block (used by lighting). |

Each heightmap stores 256 entries (one per (x,z) column) packed into longs at 9 bits per entry (enough to address a 384-block tall world).

### Chunk section

A section is written sequentially into the `Data` byte buffer. There are always exactly 24 sections, even if some are entirely empty.

| Field | Type | Notes |
|-------|------|-------|
| Non-air block count | [Short](./data-types#short) | Number of non-air block states in the section; used to short-circuit physics/render. |
| Fluid count | [Short](./data-types#short) | Number of blocks containing a fluid. |
| Block states | Paletted container | 16×16×16 entries, palette over the global block-state registry. |
| Biomes | Paletted container | 4×4×4 entries (biomes are 4-block-aligned), palette over the dynamic biome registry. |

### Paletted container

A paletted container compresses a 3D grid of registry references by carrying a small *palette* of values plus a packed-long-array of indices into that palette. The wire layout is:

| Field | Type | Notes |
|-------|------|-------|
| Bits per entry | UByte | Selects the palette format (see below). |
| Palette | format-dependent | Empty/single/list of registry IDs. |
| Data length | [VarInt](./data-types#varint) | Length of the packed long array (sometimes omitted — see below). |
| Data | long[] | Packed indices, fixed length determined by entry count and bits per entry. |

In 26.1.2 the data array is sent as a *fixed-size* long array — its length is implied by the section's entry count and the bits-per-entry, so no length prefix is written in front of it (the implementation reads `entryCount * bits / 64` longs, rounded up).

The palette format is selected by the `Bits per entry` byte:

```text
bits = 0           → SingleValuePalette
                     palette: VarInt single registry id
                     data:    no longs (every cell is the single value)

1 ≤ bits ≤ T_low   → LinearPalette (indirect)
                     palette: VarInt count, then VarInt[count] registry ids
                     data:    long[] packed indices into palette

T_low < bits < T_dir → HashMapPalette (indirect, larger)
                     palette: VarInt count, then VarInt[count] registry ids
                     data:    long[] packed indices into palette

bits ≥ T_dir       → GlobalPalette (direct)
                     palette: omitted
                     data:    long[] packed registry ids directly
```

Thresholds (`T_low`, `T_dir`) are container-specific:

- Block states: linear palette for `bits ∈ [1, 4]`, hash palette for `[5, 8]`, direct (currently 15 bits to address every block state) for `bits ≥ 9`.
- Biomes: linear palette for `bits ∈ [1, 3]`, hash palette unused, direct for `bits ≥ 4`.

The packed long array uses the standard 1.16+ "no spanning" layout: each long contains `floor(64 / bits)` entries, and entries are not split across two longs (any leftover bits are padding).

### Block entity entry

| Field | Type | Notes |
|-------|------|-------|
| Packed XZ | [Byte](./data-types#byte) | High nibble = X (0–15), low nibble = Z (0–15) within the chunk. |
| Y | [Short](./data-types#short) | Absolute world Y coordinate. |
| Type | [VarInt](./data-types#varint) | Numeric ID into the `minecraft:block_entity_type` registry. |
| Data | [NBT](./data-types#nbt) (optional) | Network NBT compound; may be empty/absent (the network NBT marker `TAG_End` indicates "no data"). |

## Light data

The light sub-payload is *also* the body of the standalone `Update Light` packet. It uses BitSets (sent as `long[]` with a length prefix) to indicate which sections include data.

| Field | Type | Notes |
|-------|------|-------|
| Sky light mask | [BitSet](./data-types#bitset) | One bit per section index (`0..lightSectionCount-1`); set if a non-empty sky-light array follows for that section. |
| Block light mask | [BitSet](./data-types#bitset) | Same, for block light. |
| Empty sky light mask | [BitSet](./data-types#bitset) | Sections that are explicitly all-dark for sky light. |
| Empty block light mask | [BitSet](./data-types#bitset) | Same, for block light. |
| Sky light arrays | Array | One `byte[2048]` per bit set in `Sky light mask`, in ascending section order. |
| Block light arrays | Array | One `byte[2048]` per bit set in `Block light mask`, in ascending section order. |

A section index runs from `0` (one section below the bottom of the world for cross-border lighting) to `lightSectionCount - 1` (one above the top), so a 24-section world emits up to 26 light bits per layer.

Each light array stores 4 bits per block (one nibble per cell, two cells per byte), totalling 4096 cells × 4 bits = 16384 bits = 2048 bytes. The lower nibble is the cell at the lower index.

A bit set in the *empty* mask means "this section exists but its values are all zero, save bandwidth"; the client should treat it as fully dark for that layer. A section absent from both the data mask and the empty mask is unchanged.

## Update Light

`Update Light` carries only:

| Field | Type | Notes |
|-------|------|-------|
| Chunk X | [VarInt](./data-types#varint) | Chunk coordinate. |
| Chunk Z | [VarInt](./data-types#varint) | Chunk coordinate. |
| Light data | sub-payload | Same layout as in `Level Chunk With Light`. |

> Source: net/minecraft/network/protocol/game/ClientboundLevelChunkPacketData.java, net/minecraft/network/protocol/game/ClientboundLightUpdatePacketData.java, net/minecraft/network/protocol/game/ClientboundLevelChunkWithLightPacket.java, net/minecraft/world/level/chunk/LevelChunkSection.java, net/minecraft/world/level/chunk/PalettedContainer.java, net/minecraft/world/level/chunk/SingleValuePalette.java, net/minecraft/world/level/chunk/LinearPalette.java, net/minecraft/world/level/chunk/HashMapPalette.java, net/minecraft/world/level/chunk/GlobalPalette.java, net/minecraft/util/SimpleBitStorage.java, net/minecraft/world/level/levelgen/Heightmap.java.
