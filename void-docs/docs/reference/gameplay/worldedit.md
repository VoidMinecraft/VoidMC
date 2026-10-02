# World Editing

The `voidmc-worldedit` crate brings a WorldEdit-style toolkit to VoidMC:
selections drawn in the world, fills and replacements, copy/paste with
rotation, undo/redo, Sponge schematics and brushes. Players need no client
mod.

It is split in two:

- an **engine-agnostic core** (regions, patterns, masks, operations, jobs,
  history, clipboard, schematics, brushes) that only talks to the world
  through the [`Extent`](#the-extent-contract) trait, and
- a thin **VoidMC adapter** (the default `engine` feature): chunk storage as
  an extent, update packets, the edit queue, sessions, the selection outline,
  the wand, brushes and the commands.

## Setup

```rust
use voidmc::VoidServer;
use voidmc_worldedit::WorldEditPlugin;

VoidServer::new(config)
    .add_plugin(|app| {
        app.add_plugins(WorldEditPlugin::default());
    })
    .run();
```

By default only players with the `Operator` component may edit. Every option
has a builder method:

```rust
WorldEditPlugin::default()
    .wand("minecraft:wooden_axe")
    .permission(Permission::Everyone)
    .blocks_per_tick(1_000_000)
    .time_per_tick(Duration::from_millis(15))
    .max_volume(32 * 1024 * 1024)
    .history(25, 128 * 1024 * 1024)
    .schematic_dir("schematics")
    .schematic_memory(256 * 1024 * 1024)
    .show_selection(true)
    .brush_preview(true);
```

| Option | Default | Meaning |
|---|---|---|
| `wand` | `minecraft:wooden_axe` | Item whose left/right clicks set the selection corners. |
| `permission` | `Operators` | `Operators` or `Everyone`. |
| `blocks_per_tick` | 1,000,000 | Work budget per tick across all queued edits (one section costs 4096). |
| `time_per_tick` | 15 ms | Wall-clock budget per tick, checked between sections. |
| `max_volume` | 33,554,432 | Largest region (bounding box) one command may touch. |
| `history` | 25 entries, 128 MiB | Per-player undo history limits. An edit whose undo data alone exceeds the memory limit is not recorded, and the player is told it cannot be undone. |
| `schematic_dir` | `schematics` | Where `/schem` reads and writes `.schem` files. |
| `schematic_memory` | 256 MiB | Memory one `/schem load` may allocate in total, and the largest file (compressed or decompressed) it reads. |
| `show_selection` | `true` | Draw the selection outline. |
| `brush_preview` | `true` | Show where the held brush would land. |

The example server (`cargo run -p voidmc-example`) gives operators the wand
when they join, unless they already carry one; set `VOID_EXAMPLE_OPERATORS=1` to make every player an
operator while testing locally.

## Commands

Coordinates are clamped to the world border (±30,000,000). Commands that
edit the world or use the clipboard (`//set`, `//paste`, `//copy`, `//undo`,
`//rotate`, `/schem save`, …) are refused while your previous edit is still
running.

Commands follow WorldEdit: most start with a double slash because the
command itself is named `/set`, `/copy`, …

| Command | Effect |
|---|---|
| `//wand` | Get the wand. Left click a block: first corner (the block is kept). Right click: second corner. |
| `//pos1 [x y z]`, `//pos2 [x y z]` | Set a corner to your feet or to the given block (`~` works). |
| `//hpos1`, `//hpos2` | Set a corner to the block you look at. |
| `//sel [cuboid\|sphere\|cyl]` | Choose the selection shape; no argument clears it. |
| `//desel` | Clear the selection. |
| `//size` | Dimensions and block count of the selection. |
| `//expand <n> [dir]`, `//expand vert` | Push the face towards `dir` out by `n`; `vert` spans the full world height. |
| `//contract <n> [dir]` | Pull the face opposite `dir` towards it by `n`. |
| `//shift <n> [dir]` | Move the selection, not the blocks. |
| `//set <pattern>` | Fill the selection. |
| `//replace [mask] <pattern>` | Replace matching blocks (default mask: any non-air block). |
| `//walls <pattern>`, `//outline <pattern>` (`//faces`) | Four sides / all six faces of a cuboid selection. |
| `//move [n] [dir] [-s] [-a]` | Move the blocks; `-s` moves the selection along, `-a` skips air. |
| `//stack [count] [dir] [-s] [-a]` | Repeat the selection next to itself; `-s` selects the last copy. |
| `//copy`, `//cut` | Copy (and clear) the selection, anchored at your position. |
| `//paste [-a] [-o] [-s]` | Paste at your position; `-o` at the original position, `-s` selects the result. |
| `//rotate <90\|180\|270>` | Turn the clipboard clockwise seen from above, block states included. |
| `//flip [dir]` | Mirror the clipboard along a direction. |
| `//undo [n]`, `//redo [n]`, `//clearhistory` | History. Undo restores blocks, not the contents of chests, signs or other block entities. |
| `/schem save\|load\|delete <name>`, `/schem list` (`/schematic`) | Sponge schematics in `schematic_dir`; `save` overwrites. |
| `/brush …` (`/br`) | Bind a brush to the held item, see [Brushes](#brushes). |

`dir` is `north`, `south`, `east`, `west`, `up`, `down` (or `n`/`s`/…) and
defaults to `me`, the way you look. Tokens starting with `-` are flags, so
use the opposite direction instead of a negative amount.

### Patterns and masks

A **pattern** says what to place:

- `stone`, `minecraft:stone` — one block, default state;
- `oak_stairs[facing=east,half=top]` — a state, unset properties default;
- `50%stone,30%dirt,gravel` — a weighted mix. Weights are relative; an entry
  without one weighs 1. Picks hash the position, so a mix is reproducible for
  a given edit.

A **mask** says which existing blocks may change:

- `stone,dirt` — any state of those blocks;
- `oak_stairs[facing=east]` — exactly that state;
- `#existing` — any non-air block; `*` — anything;
- `!mask` — the opposite.

Patterns and masks complete block names while you type.

## Selections and the outline

The selection is drawn with display entities that exist **only on the owner's
client**: two glowing markers on the corners and twelve thin glowing edges,
visible through walls. They are sent straight to the player (spawn, metadata,
teleport, remove) and never enter the server's entity tracker, so other
players cannot see them and they cost nothing per tick. When the selection
changes, edges glide to their new place with client-side interpolation.

Spheres and cylinders are outlined by their bounding box. A selection is not
tied to a dimension: it follows you if you change worlds.

## Brushes

Hold any item without its own behaviour and bind a brush to it:

| Brush | Effect |
|---|---|
| `/brush sphere [-h] <pattern> [radius]` | Fill (or `-h` shell) a sphere. |
| `/brush cyl [-h] <pattern> [radius] [height]` | Fill a cylinder standing on the target (height up to 64). |
| `/brush smooth [radius] [iterations]` | Blur the terrain heightmap. |
| `/brush gravity [radius]` | Drop every block of the sphere onto what is below it. |
| `/brush mask <mask>` | Limit the bound brush to matching blocks. |
| `/brush range <blocks>` | How far the brush reaches (default 128). |
| `/brush none` | Unbind. |

Right click fires the brush at the first block in sight. While a brush item
is held, a ring of particles shows where it would land; a second ring flashes
when it fires. Radii are capped at 32. Binding refuses items that already
have an `ItemBehavior` (and the wand), so a brush never replaces another
plugin's behaviour.

## Schematics

`/schem save <name>` writes the clipboard as a gzip-compressed **Sponge
schematic v3** (`<schematic_dir>/<name>.schem`); `/schem load <name>` reads
v1, v2 or v3 files (WorldEdit and FAWE exports included) into the clipboard.
The offset between the paste origin and the copied volume survives the round
trip, so a building pastes where it should relative to you. Block names
unknown to this server become air and are listed. Block entities, entities
and biomes are not carried over.

Saving, loading, `//rotate` and `//flip` run on a background thread, so a
large clipboard never stalls the tick; the result is applied on a later tick.
Until then your other edits are refused with "Your previous edit is still
running", and at most four such tasks run at once across the server. A load
streams the file instead of building its whole NBT tree: fields the clipboard
does not use (biomes, entities, block entities, anything unknown) are skipped
without being stored, and the block data is decoded straight into the
clipboard. Everything the load allocates (decompression state, the decoded
blocks, the palette) counts against one `schematic_memory` budget, the file
may not decompress to more than that, and data after the root compound is
rejected; a crafted file is refused within about a second whatever its shape.
Saves go to a temporary file that is renamed into place, so a reader never
sees a half-written schematic.

```rust
use voidmc_worldedit::Schematic;

let schematic = Schematic::load("castle.schem")?;
println!("{} unknown blocks", schematic.unknown_blocks.len());
Schematic::save(&schematic.clipboard, "castle-copy.schem")?;
```

## Performance

Editing never freezes the tick:

- **Edits run as jobs**, section by section, inside a per-tick budget
  (`blocks_per_tick` and `time_per_tick`). A multi-million block `//set`
  spreads over several ticks, with a progress line on the action bar.
- **Sections are rewritten in bulk.** A job reads a section into a 4096-entry
  buffer, the operation edits it in place, and the adapter re-encodes the
  palette once: a fully covered uniform section becomes a single-value
  palette, large palettes switch to global ids like vanilla.
- **Updates are batched.** Each changed section goes out as one
  `section_blocks_update` (`block_update` when only one block changed); a
  chunk with eight touched sections, or six sections' worth of changed
  blocks, in one tick is resent whole.
- **History is compact.** Each changed section is stored as runs of
  `(length, old, new)`, so filling a section of air with stone is one run.
- Patterns and masks are allocation-free: masks are bitsets over every block
  state, random patterns hash the position.

Criterion benchmarks (`cargo bench -p voidmc-worldedit --bench edits`) run
the core against an in-memory extent. On an Apple-silicon laptop:

| Benchmark | Time |
|---|---|
| set 1M blocks, one block | ~3 ms |
| set 1M blocks, weighted random pattern | ~10 ms |
| replace with a mask over 1M blocks | ~11 ms |
| undo a 1M block edit | ~11 ms |
| sphere of radius 40 (~270k blocks) | ~1.6 ms |

Edits only touch loaded chunks; unloaded sections are skipped and reported.
Per-block events (`BlockChangeEvent`, …) are not fired for edits, but changed
chunks are marked `ChunkDirty` for persistence.

## Programmatic use

The plugin's queue runs any [`Operation`](#the-extent-contract) in a
dimension. Give it an owner to record it in their history and tell them when
it is done:

```rust
use voidmc_worldedit::engine::{Edit, EditQueue};
use voidmc_worldedit::{BlockPos, Cuboid, Fill, Pattern};

fn build_floor(world: &mut World, player: Entity) {
    let floor = Cuboid::new(BlockPos::new(-50, 63, -50), BlockPos::new(50, 63, 50));
    let pattern = Pattern::parse("stone,andesite").unwrap();
    world.resource_mut::<EditQueue>().submit(
        Edit::new(DimensionId::Overworld, Fill::new(floor, pattern))
            .owner(player)
            .label("Floor"),
    );
}
```

`EditSession` (a component on players who edited) exposes the selection,
clipboard, history and bound brushes.

## The extent contract

The core sees the world only through `Extent`:

```rust
pub trait Extent {
    fn height(&self) -> (i32, i32);
    fn block(&self, pos: BlockPos) -> Option<BlockState>;
    fn read_section(&self, section: SectionPos, out: &mut SectionBlocks) -> bool;
    fn write_section(&mut self, section: SectionPos, blocks: &SectionBlocks, changed: &ChangeMask);
}
```

Implement it on another storage and every operation, undo, copy and brush
works unchanged; `MemoryExtent` is a ready-made in-memory one for tests and
offline tools. Without the `engine` feature the crate does not depend on the
server at all:

```rust
use voidmc_worldedit::{BlockPos, BlockState, Cuboid, EditJob, Extent, Fill, MemoryExtent, Restore};

let mut world = MemoryExtent::default();
let region = Cuboid::new(BlockPos::new(0, 0, 0), BlockPos::new(99, 99, 99));
let (changes, stats) = EditJob::new(Fill::new(region, BlockState::parse("stone")?), world.height())
    .run(&mut world);
EditJob::new(Restore::undo(changes.into()), world.height()).run(&mut world);
```

## Not yet supported

- The WorldEditCUI channel (`worldedit:cui`): the engine has no play-state
  custom payload yet.
- Polygon selections; spheres and cylinders are outlined by their bounds.
- Block entities, entities and biomes in clipboards and schematics.
- Lighting and heightmaps are not recomputed after an edit.
- Undo does not restore block-entity contents (chest items, sign text).
- No `//cancel`; edits run first in, first out.
