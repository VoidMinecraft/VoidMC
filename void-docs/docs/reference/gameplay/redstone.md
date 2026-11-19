# Basic Redstone

`RedstonePlugin` adds enough redstone to trigger note blocks from a lever or a
button, directly or through a line of redstone dust. It is opt-in: it is not
part of `DefaultPlugins`, because it changes what a right-click on a lever or
button does (without it, the held block is placed against it as on any other
block), and a server that does not want world simulation should not pay for
it.

```rust
use voidmc::plugins::redstone::RedstonePlugin;

VoidServer::new(config)
    .add_plugin(|app| {
        app.add_plugins(RedstonePlugin);
    })
    .run();
```

The example server enables it, and its `/circuit` command builds a lever, three
dust and a note block, plus a stone button next to a second note block, two
blocks east of the player.

## What is supported

| Component | Behaviour |
|---|---|
| Lever (any `face` and `facing`) | A main-hand right-click toggles `powered` and plays `block.lever.click` to everyone near (pitch 0.6 on, 0.5 off). |
| Buttons: stone, polished blackstone, every wooden button | A right-click on an unpowered button powers it; it releases by itself 20 game ticks later (stone, polished blackstone) or 30 (wood). A click on a pressed button does nothing. Click on/off sounds as in vanilla (the presser's client plays its own click). |
| Powered lever or button | Powers its six neighbours with level 15. |
| Redstone dust (`redstone_wire`), flat | Power 15 next to a powered lever or button, minus one per dust along horizontally adjacent dust on the same level: the 15th dust has power 1, the 16th has 0. Dust powers the block below it and the horizontal neighbours it points into. |
| Note block | Powered by a powered lever or button next to it, a powered dust on top of it, or a powered dust beside it that points into it. |

When a note block's input goes from unpowered to powered, the server stores
`powered=true` and plays its note once: a Sound packet
(`block.note_block.<instrument>`, category `Records`, volume 3, pitch
`2^((note - 12) / 12)`, from the block's centre) and a Block Event packet
(action 0, parameter 0, block `minecraft:note_block`) for the note particle,
both to the players holding the chunk. It does not play again while the input
stays on, nor when it turns off (`powered=false` is stored and sent); it plays
on the next rising edge. A note block with a non-air block directly above it
stays silent, like vanilla, unless its instrument is a mob head.

The instrument and the note are read from the note block's block state. Vanilla
sets the instrument from the block below when the note block is placed or a
neighbour changes; VoidMC does not recompute it, and placing a note block puts
the default (harp, note 0). Tuning by right-clicking a note block is not
implemented.

Every state change is a normal block change: it is stored in the chunk, so a
player who joins or rejoins receives the current lever, dust and note-block
states, it is broadcast as a Block Update to every player holding the chunk,
and it fires `BlockChangeEvent`.

### Dust shape

Which neighbours a dust points into is computed from the blocks around it,
like vanilla's connection shape restricted to one level: dust connects to
adjacent dust, levers and buttons; a dust connected along one axis only also
points out the other end of that axis (a straight line points into the block
past its end); a dust with no connection points everywhere, unless it was
placed as a dot (all connections `none`), in which case it points nowhere.

Only the `power` property is written. The connection properties (`north`,
`east`, `south`, `west`) stay as they were placed, since VoidMC's block
placement does not compute shapes yet: prebuilt circuits should use the
vanilla shapes.

## What is not supported

- Dust going up or down a block (staircases, dust on the side of a block).
- Power through solid blocks: a lever does not strongly power the block it is
  attached to, and dust pointing into a stone block does not power what is
  behind it. A lever on the side of a note block works because it is adjacent.
- Redstone torches, repeaters, comparators, observers, pistons, redstone
  blocks, pressure plates, target blocks, daylight sensors, and any other
  component or complex circuit.
- Changes made without a `BlockChangeEvent` (writing `ChunkData` directly, as
  WorldEdit operations do) are not seen until another change happens nearby.

## Propagation

Each block change involving a lever, button, dust or note block queues its
position (`Redstone::queue_change` does the same by hand). Once per tick, in
`Update` under `VoidSystems::Redstone` (after `ItemUseDrain`, `CommandDrain`
and `MenuClickDrain`), the plugin:

1. releases the buttons whose delay has run out;
2. for each queued change, recomputes the dust within 15 dust steps of it from
   the levers and buttons reachable within 30 steps, and writes the dust whose
   power changed;
3. re-evaluates the note blocks next to each change and next to each dust whose
   power changed, like vanilla's neighbour updates: a note block that was just
   placed next to powered dust is not notified by its own placement.

The work per change is bounded, the traversal is iterative (no recursion), a
loop of dust is visited once, and blocks in chunks that are not loaded are
treated as absent: they are never read through or written. At most
`MAX_CHANGES_PER_TICK` (256) changes are processed in one tick; the rest wait
for the next tick. A lever toggled on then off within the same tick produces no
note.

## Right-clicks

Levers and buttons are registered in `BlockInteractionRegistry` (see
[Items & Inventory](./items.md#blocks-that-react-to-a-right-click)). An item
with its own `ItemBehavior`, such as the WorldEdit wand or the navigation
remote, keeps priority over the lever. Otherwise, as in vanilla, a main-hand
right-click toggles the lever whatever the player holds (nothing is placed),
except when the player is sneaking with an item in either hand: then the item
is used. An off-hand use never toggles a lever, so a click that a client sends
as a main-hand then an off-hand use toggles it once. Every use is acknowledged
with Block Changed Ack.
