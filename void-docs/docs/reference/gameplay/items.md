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

The full vanilla click set is implemented server-authoritatively
(`ClickContainer`): pickup (left/right split & merge), shift quick-move, number-
key/offhand swap, creative clone, throw, multi-slot drag, and double-click
pickup-all. A click carrying a stale state id is answered with a full resync,
as in vanilla.

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

Three hooks are available:

| Method | Fires on |
|--------|----------|
| `on_use_on_block` | right-click pointing at a block |
| `on_use` | right-click in the air |
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
