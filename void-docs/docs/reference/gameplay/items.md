# Items & Inventory

Void models items as real `ItemStack`s, gives every player a 46-slot inventory
synced to the client, and lets you override what any item does — placement,
right-click, and break — without touching the framework.

## ItemStack and ItemId

`ItemId` wraps a `minecraft:item` registry id; `ItemStack` is an id + count +
data components.

```rust
use voidmc::{ItemId, ItemStack};

let stone = ItemId::from_name("minecraft:stone").unwrap();
assert!(stone.is_block_item());                 // places a block when used
assert_eq!(stone.default_block_state(), Some(/* state id */ 1));

let stack = ItemStack::of("minecraft:diamond_block", 5).unwrap();
```

`ItemStack` converts to/from the protocol `Slot` via `to_slot` / `from_slot`;
everything else in the server works with `ItemStack`.

## The player Inventory

Each player carries an `Inventory` component (window id 0). Slot layout:

| Slots | Region |
|-------|--------|
| 0 | crafting output |
| 1–4 | crafting grid |
| 5–8 | armor (head, chest, legs, feet) |
| 9–35 | main inventory |
| 36–44 | hotbar (key `k` → slot `36 + k`) |
| 45 | offhand |

```rust
use voidmc::Inventory;

fn give_kit(mut q: Query<&mut Inventory>, player: Entity) {
    if let Ok(mut inv) = q.get_mut(player) {
        let leftover = inv.give(ItemStack::of("minecraft:stone", 64).unwrap());
        // `leftover` is what didn't fit. `inv.held()` is the selected hotbar item.
    }
}
```

Per-item stack caps are respected (`give` of 20 ender pearls leaves a 16 + 4
split).

### Syncing

There is nothing to flag. Every setter (`set`, `set_cursor`, `give`, `clear`,
`set_selected_hotbar`, the click handlers) records what it changed, and the
`InventorySync` phase in `PostUpdate` sends the smallest matching packets once
per tick: one `SetContainerSlot` per changed slot, `SetCursorItem` for the
cursor, `SetHeldSlot` for the selection, and a full `SetContainerContent` only
when more than half the window changed or `clear` was called. Setting a slot to
the value it already holds sends nothing. While a [menu](menus.md) is open,
main-inventory and hotbar changes go through the menu window instead; the rest
is resent when the menu closes.

### Clicks

Container clicks on window 0 are applied server-side with vanilla's rules. A
stack's max size is its item's, or its `max_stack_size` component if it has
one. An armor slot (5–8) only takes an item whose `equippable` component
names that slot (the helmets, carved pumpkin and mob heads on the head, and
so on; the per-item data comes from Mojang's item components report, see
`void-data`), and holds one item; every click below follows that rule. The
off-hand takes any item.

| Click | Effect |
|---|---|
| Left / right click | Picks up the whole stack / half of it (rounded up); places the whole cursor / one item; merges into the same item up to its max stack size, the rest staying on the cursor; swaps with a different item |
| Click outside the window (slot `-999`) | Drops the cursor stack (left) or one item of it (right) as an `ItemDropEvent` |
| Shift-click | An item equippable in an empty armor slot goes there, and one equippable in the off-hand (a shield) to an empty off-hand, unless it comes from the crafting grid or an armor slot. Otherwise: hotbar to main inventory and back; crafting grid, armor and off-hand slots to the main inventory then the hotbar; the crafting output to the hotbar from the right, then the main inventory from the bottom. Partial stacks fill first, then empty slots; what does not fit stays. As in vanilla the move repeats while it moves something, so a stack of pumpkins puts one on an empty head and the rest moves on to the hotbar or main inventory |
| Number key 1–9 / F | Swaps the hovered slot with that hotbar slot / the off-hand. A stack too large for the hovered armor slot leaves one item there; what the slot held goes back into the inventory (hotbar first), dropped if it does not fit |
| Drag (left / right) | Spreads the cursor evenly over the dragged slots (the remainder stays on the cursor) / places one per slot. A slot holding another item is skipped, a slot named twice counts once, a drag never covers more slots than the cursor has items, and a packet out of order, an empty cursor or any other click cancels the drag |
| Double-click | Fills the cursor with the same item from the whole window except the crafting output, partial stacks before full ones, in slot order (from the end with button 1), if the clicked slot is empty |
| Drop key Q / Ctrl+Q over a slot | Drops one item / the whole stack, only with an empty cursor |
| Creative clone (middle click), creative middle-drag | A full stack on the cursor / in each dragged slot, the cursor keeping its stack; only when the server's `game_mode` is creative |

An unknown click type (decoded as a pickup), an unknown button, a slot index
outside the window or a click on another window id changes nothing. Nothing
can be placed into the crafting output (slot 0), which only gives its whole
stack. Closing the inventory returns the cursor stack and the crafting grid
to the inventory, dropping what does not fit.

Not supported: nothing is crafted in the crafting grid, and only an item's
default `equippable` component is known (a stack's own component is not
read). A vanilla client predicts crafting differently and is corrected by the
resync below.

The client sends what it expects each slot it changed and its cursor to hold
(hashed). As in vanilla, the server compares that prediction with its own
result: a correct prediction is not echoed, while a slot or cursor predicted
wrong, or a slot the server changed but the client did not name, is resent
with `SetContainerSlot` / `SetCursorItem`. The server does not compute
component hashes, so a stack with data components is always resent. A click
carrying a stale state id is answered with a full `SetContainerContent`.

`SetCreativeModeSlot` is only honoured when the server's `game_mode` is
creative, for slots 1–45 and stacks within their max size; otherwise the
slot's true contents are resent. As in vanilla, it ignores the armor-slot
rule: a creative player can put any item on its head. On a creative server a
negative slot throws the stack, if it is within its max size ([Dropped
items](#dropped-items)); on any other server it does nothing.

### Equipment seen by other players

Other players are shown what a player holds and wears with Set Equipment:
the selected hotbar slot as the main hand, the off-hand and the four armor
slots. As in vanilla, the `PlayerBroadcast` phase in `PostUpdate` sends a
slot to every other ready player (the players its entity is spawned for) when
it changes, whatever changed it (selecting another hotbar slot, `/give`,
`/clear`, clicks, creative slot sets, drops, pickups or the `Inventory` API),
and every non-empty slot to a player who joins. A player is never sent its own
equipment, and empty equipment is never sent to a joining player. Swapping
hands with the F key outside the inventory screen is not handled by the
server.

### Held slot and cooldowns

`Inventory::selected_hotbar()` is the one source of truth for the selected
hotbar key; the client's own changes land there through
`PlayerChangeSlotEvent`. To move the selection from the server use the
`Inventories` system param (or `WorldInventories` from `&mut World`):

```rust
use voidmc::{Cooldown, Inventories, ItemId};

fn on_use(mut inventories: Inventories, player: Entity) {
    inventories.set_held_slot(player, 3);
    let pearl = ItemId::from_name("minecraft:ender_pearl").unwrap();
    inventories.cooldown(player, Cooldown::item(pearl).ticks(20));
}
```

`Cooldown::item` uses the item's own cooldown group; `Cooldown::group("name")`
targets a shared group (`use_cooldown` component). `ticks(0)` clears it.

## Overriding item behaviour

Implement `ItemBehavior` for a unit struct and register it for an item. Methods
default to `UseResult::Pass`, which runs the built-in default (a block item
places its block); return `UseResult::Handled` to take over.

```rust
use voidmc::{
    ItemBehavior, ItemBehaviorRegistry, ItemUseContext, ServerConfig, UseResult, VoidServer,
};

struct Wand;
impl ItemBehavior for Wand {
    fn on_use_on_block(&self, ctx: &mut ItemUseContext) -> UseResult {
        ctx.reply("zap!");
        ctx.place_block(voidmc_data::v26_1_2::blocks::GLOWSTONE);
        UseResult::Handled
    }
}

VoidServer::new(ServerConfig::default())
    .add_plugin(|app| {
        app.world_mut()
            .resource_mut::<ItemBehaviorRegistry>()
            .register_for("minecraft:stick", Wand);
    })
    .run();
```

Block-state constants such as `GLOWSTONE` come from the `voidmc-data` crate, so
add it next to `voidmc`; `ItemId::from_name("minecraft:glowstone")` and
`default_block_state()` give the same id without it. Registering a second
behaviour for the same item replaces the first. `register_for` resolves the name
when it is called: an unknown name logs a warning and registers nothing.

`ItemUseContext` exposes full world access — `place_block`, `set_block`,
`give`, `consume`, `reply`, and `with_world` / `with_world_mut`. Behaviours run
in an exclusive system (the same queue + drain pattern as commands), so they can
mutate anything.

A right-click runs the behaviour of the item in the hand that was used
(`ctx.hand`): the selected hotbar slot for the main hand, slot 45 for the off
hand. `ctx.held` is that item as it was when the packet arrived, and `consume`
takes from that slot. The client tries the main hand first and the off hand
next, unless it expects the main-hand item to do something. A main-hand
behaviour the client does not know about can therefore be followed by an
off-hand use of the same click, which runs the off-hand item. Before that, the
client follows the use on the block with a use in the air of the same hand, so a
behaviour that implements both `on_use_on_block` and `on_use` runs both for one
click on a block. An empty hand runs no item behaviour and places nothing; the
clicked block's own interaction ([below](#blocks-that-react-to-a-right-click))
still runs, so an empty hand toggles a lever.

Four hooks are available:

| Method | Fires on |
|--------|----------|
| `on_use_on_block` | right-click pointing at a block |
| `on_use` | right-click in the air |
| `before_break_block` | when the player starts breaking a block, and in survival again right before the mined block is removed; `Handled` keeps it and resyncs the player |
| `on_break_block` | after the player breaks a block (tool side effects) |

Overriding the default placement is just a `Handled` `on_use_on_block`:

```rust
struct DirtToBedrock;
impl ItemBehavior for DirtToBedrock {
    fn on_use_on_block(&self, ctx: &mut ItemUseContext) -> UseResult {
        ctx.place_block(voidmc_data::v26_1_2::blocks::BEDROCK);
        UseResult::Handled
    }
}
// reg.register_for("minecraft:dirt", DirtToBedrock);
```

## Blocks that react to a right-click

A right-click on a block runs, in order: the held item's `ItemBehavior`, then
the clicked block's `BlockInteraction` (if the item passed), then the default
placement (if both passed). Register an interaction for every state of a
block in `BlockInteractionRegistry`; it is empty by default, and the opt-in
[`RedstonePlugin`](./redstone.md) fills it for levers and buttons.

```rust
use voidmc::{BlockInteraction, BlockInteractionRegistry, ItemUseContext, UseResult};

struct Doorbell;
impl BlockInteraction for Doorbell {
    fn on_use(&self, ctx: &mut ItemUseContext, _state: i32) -> UseResult {
        ctx.reply("ding dong");
        UseResult::Handled // consumed: the held block is not placed
    }
}
// app.world_mut().resource_mut::<BlockInteractionRegistry>()
//     .register_for("minecraft:bell", Doorbell);
```

As in vanilla, the block's interaction only runs for a main-hand use, and not
while the player is sneaking with an item in either hand (`Sneaking`
component, from the Player Input packet): then the item is used, so a sneaking
player can place a block against a lever. An off-hand use, which a client sends
after a main-hand use it did not consume, never reaches the block.

## Getting items in

- **Creative**: grabbing an item in the creative menu (`SetCreativeModeSlot`)
  stores it server-side, so it can be placed.
- **`/give <item> [count]`** and **`/clear`** commands (registered by
  `register_default_commands`).
- **Programmatically** via `Inventory::give` or `ItemUseContext::give`.

## Dropped items

The drop key (`Q` for one item, `Ctrl+Q` for the stack), a throw click in the
inventory, a stack clicked outside the inventory window, and a stack thrown out
of the creative inventory each spawn one `minecraft:item` entity carrying
exactly what left the inventory. Emit a `voidmc::events::ItemDropEvent {
dropper, stack }` to drop an item from your own code; it is thrown the same
way.

- **Throw.** The item leaves 1.32 blocks above the dropper's feet (0.3 below a
  standing player's eyes) at 0.3 blocks per tick in the direction they look,
  plus 0.1 upwards, like vanilla without its random spread. Looking straight
  down drops it at the feet.
- **Flight.** It falls with the entity physics of `--gravity --block-checks`
  summons, keeps 98% of its horizontal speed per tick in the air and 58.8% on
  the ground, and comes to rest on the block top. A throw on flat ground lands
  about two blocks ahead.
- **Rendering.** Every viewer, including players who come into range later, is
  sent the add-entity and the entity metadata holding the stack.
- **Pickup delay.** `voidmc::plugins::item_drops::PICKUP_DELAY_TICKS` (40, vanilla's two seconds). An item
  thrown during tick `T` can be collected from tick `T + 40` on, also by the
  player who threw it.
- **Pickup.** A ready player whose feet are within `PICKUP_RADIUS` (1.5 blocks)
  of the item's position collects it. The stack goes in with `Inventory::give`:
  onto matching stacks first, then into empty slots, hotbar before main. Viewers
  are sent the pickup animation (Take Item Entity) with the amount taken. A
  player whose inventory is full takes nothing and leaves the item to the
  next player in range; one with room for part of it takes that part and the
  rest stays on the ground, with its metadata updated. Players in range in the
  same tick take in turn, so one item is never given twice.

## Events

Observe these to react to inventory/item activity:

- `BlockPlaceEvent` / `BlockBreakEvent` — committed world changes.
- `ItemDropEvent` — an item being dropped into the world.
- `PlayerChangeSlotEvent` — the client changed its selected hotbar slot.
- `MenuClickEvent` / `MenuClosedEvent` — see [Menus](menus.md).
