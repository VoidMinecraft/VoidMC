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
one.

| Click | Effect |
|---|---|
| Left / right click | Picks up the whole stack / half of it (rounded up); places the whole cursor / one item; merges into the same item up to its max stack size, the rest staying on the cursor; swaps with a different item |
| Click outside the window (slot `-999`) | Drops the cursor stack (left) or one item of it (right) as an `ItemDropEvent` |
| Shift-click | Hotbar to main inventory and back; crafting grid, armor and off-hand slots to the main inventory then the hotbar; the crafting output to the hotbar from the right, then the main inventory from the bottom. Partial stacks fill first, then empty slots; what does not fit stays |
| Number key 1–9 / F | Swaps the hovered slot with that hotbar slot / the off-hand |
| Drag (left / right) | Spreads the cursor evenly over the dragged slots (the remainder stays on the cursor) / places one per slot. A slot holding another item is skipped, a slot named twice counts once, a drag never covers more slots than the cursor has items, and a packet out of order, an empty cursor or any other click cancels the drag |
| Double-click | Fills the cursor with the same item from the whole window except the crafting output, partial stacks before full ones, in slot order (from the end with button 1), if the clicked slot is empty |
| Drop key Q / Ctrl+Q over a slot | Drops one item / the whole stack, only with an empty cursor |
| Creative clone (middle click), creative middle-drag | A full stack on the cursor / in each dragged slot, the cursor keeping its stack; only when the server's `game_mode` is creative |

An unknown click type (decoded as a pickup), an unknown button, a slot index
outside the window or a click on another window id changes nothing. Nothing
can be placed into the crafting output (slot 0), which only gives its whole
stack. Closing the inventory returns the cursor stack and the crafting grid
to the inventory, dropping what does not fit.

Not supported: the server has no equipment data for items, so armor and the
off-hand slots accept any item, and shift-clicking armor or a shield moves it
like any other item instead of equipping it. Nothing is crafted in the
crafting grid. A vanilla client predicts these differently and is corrected
by the resync below.

The client sends what it expects each slot it changed and its cursor to hold
(hashed). As in vanilla, the server compares that prediction with its own
result: a correct prediction is not echoed, while a slot or cursor predicted
wrong, or a slot the server changed but the client did not name, is resent
with `SetContainerSlot` / `SetCursorItem`. The server does not compute
component hashes, so a stack with data components is always resent. A click
carrying a stale state id is answered with a full `SetContainerContent`.

`SetCreativeModeSlot` is only honoured when the server's `game_mode` is
creative, for slots 1–45 and stacks within their max size; otherwise the
slot's true contents are resent.

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
use voidmc::{ItemBehavior, ItemBehaviorRegistry, ItemUseContext, UseResult, VoidServer};

struct Wand;
impl ItemBehavior for Wand {
    fn on_use_on_block(&self, ctx: &mut ItemUseContext) -> UseResult {
        ctx.reply("zap!");
        ctx.place_block(voidmc_data::v26_1_2::blocks::GLOWSTONE);
        UseResult::Handled
    }
}

VoidServer::new(config)
    .add_plugin(|app| {
        app.world_mut()
            .resource_mut::<ItemBehaviorRegistry>()
            .register_for("minecraft:stick", Wand);
    })
    .run();
```

`ItemUseContext` exposes full world access — `place_block`, `set_block`,
`give`, `consume`, `reply`, and `with_world` / `with_world_mut`. Behaviours run
in an exclusive system (the same queue + drain pattern as commands), so they can
mutate anything.

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

## Getting items in

- **Creative**: grabbing an item in the creative menu (`SetCreativeModeSlot`)
  stores it server-side, so it can be placed.
- **`/give <item> [count]`** and **`/clear`** commands (registered by
  `register_default_commands`).
- **Programmatically** via `Inventory::give` or `ItemUseContext::give`.

## Dropped items

Throwing from the inventory or the drop key spawns a `minecraft:item` entity
that falls and can be picked up by nearby players. Emit a `voidmc::events::
ItemDropEvent { dropper, stack }` to drop an item from your own code.

## Events

Observe these to react to inventory/item activity:

- `BlockPlaceEvent` / `BlockBreakEvent` — committed world changes.
- `ItemDropEvent` — an item being dropped into the world.
- `PlayerChangeSlotEvent` — the client changed its selected hotbar slot.
- `MenuClickEvent` / `MenuClosedEvent` — see [Menus](menus.md).
