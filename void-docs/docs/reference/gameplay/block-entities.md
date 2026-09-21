# Block Entities

A block entity is the extra data some blocks carry: the text of a sign, the
skin of a player head, the layers of a banner, the name of a chest. VoidMC
stores it in the chunk next to the block, sends it with the chunk, pushes
changes to the players seeing the chunk, and persists it with the chunk.

## Placing a sign

```rust
use voidmc::world::{BlockMutation, mutate_block};
use voidmc::{BlockFace, BlockPosition, DimensionId, DyeColor, Sign, SignSide, set_block_entity};
use voidmc_data::v26_1_2::blocks;

fn welcome(world: &mut World, actor: Entity, dimension: DimensionId, position: BlockPosition) {
    mutate_block(
        world,
        actor,
        dimension,
        position,
        blocks::OAK_SIGN,
        BlockFace::Top,
        BlockMutation::Place,
    );
    set_block_entity(
        world,
        dimension,
        position,
        Sign::lines(["Welcome", "to", "Void", ""])
            .back(SignSide::lines(["", "", "", ""]).color(DyeColor::Red).glowing())
            .waxed(),
    )
    .unwrap();
}
```

The block must already host that kind of block entity: `set_block_entity`
returns `BlockEntityError::WrongBlock` for a sign on stone, `ChunkNotLoaded`
when the chunk is not in memory and `OutsideWorld` for an impossible `y`. It
returns the block entity it replaced, if any. `/sign <text...>` in
`void-example` is the runnable version of the snippet above.

## API

| Call | Where | Does |
|---|---|---|
| `set_block_entity(world, dimension, position, block_entity)` | `&mut World` | Store (or replace) and schedule a Block Entity Data packet. |
| `block_entity_at(world, dimension, position)` | `&World` | Read. |
| `remove_block_entity(world, dimension, position)` | `&mut World` | Remove; viewers get the block entity reset to its defaults (a blank sign). |
| `BlockEntities` (`SystemParam`) | systems | `get`, `set`, `remove` with the same semantics. |
| `ChunkData::block_entity(chunk, pos)` / `set_block_entity(chunk, pos, be)` / `remove_block_entity(chunk, pos)` / `block_entities(chunk)` | chunk | The storage the above delegate to; `chunk` is the column the `ChunkData` belongs to (checked in debug builds). |

Changing a block to one that cannot host the stored kind removes the block
entity automatically — breaking a sign never leaves its text behind, and
turning an oak sign into a spruce sign keeps it. Through `mutate_block` (the
live path) the Block Update packet already tells the client, so no extra
packet is sent; a direct `ChunkData::set_block` (generation, bulk edits) drops
the entry without notifying anyone.

Changes are flushed once per tick in `PostUpdate`
(`VoidSystems::BlockEntitySync`, before `ChunkStreaming`) to
`Recipients::seeing_chunk`; players who receive the chunk itself get the block
entities inside the chunk packet. Every change also marks the chunk
`ChunkDirty`, so [world serialization](../server/world-serialization.md) saves
block entities as an Anvil-shaped `block_entities` list (`id`, `x`, `y`, `z`
plus the entity's own tags).

## Typed constructors

| Type | Builder | Block entity kind |
|---|---|---|
| `Sign` | `Sign::lines([..; 4])`, `.front(SignSide)`, `.back(SignSide)`, `.waxed()`, `.hanging()` | `minecraft:sign` (`minecraft:hanging_sign` with `.hanging()`) |
| `SignSide` | `SignSide::lines([..; 4])`, `.color(DyeColor)`, `.glowing()` | one face of a sign |
| `Skull` | `Skull::player(name)`, `Skull::texture(base64)`, `.name()`, `.id(Uuid)` | `minecraft:skull` |
| `Banner` | `Banner::new(base: DyeColor)`, `.layer(BannerPattern, DyeColor)`, `.block_state()` | `minecraft:banner` |
| `BlockEntity::raw(kind, Compound)` | any NBT | any `BlockEntityKind` |

Sign lines are plain text; exactly four per side, `""` for a blank line. A
banner's base colour is the block, not the NBT — `Banner::block_state()` gives
the standing banner state to place. `BannerPattern::named("stripe_top")`
resolves against the `minecraft:banner_pattern` registry and returns `None`
for unknown patterns. `BlockEntityKind::from_name` / `for_block_state` resolve
against the `minecraft:block_entity_type` registry and the block-state ranges
shipped by `voidmc_data`; the NBT schemas follow Paper 26.1.2
(`SignText`, `ResolvableProfile`, `BannerPatternLayers`).

## Not a display entity

A block entity only exists on a block that hosts it, at a block position, and
the client renders it as part of that block. Floating text, a rotated item or
a block scaled in mid-air is a *display entity* (`text_display`,
`item_display`, `block_display`): a real entity with an id, a position in
doubles and metadata, spawned and removed like any other entity. Use a block
entity for what a block *is* (this sign says "Welcome"); use a display entity
for what floats *near* a block.
