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
split). After mutating an inventory outside the built-in handlers, insert the
`voidmc::plugins::inventory::InventoryDirty` marker to re-sync the client.

The full vanilla click set is implemented server-authoritatively
(`ClickContainer`): pickup (left/right split & merge), shift quick-move, number-
key/offhand swap, creative clone, throw, multi-slot drag, and double-click
pickup-all.

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
- `PlayerChangeSlotEvent` — the selected hotbar slot changed.
