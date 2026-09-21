# Menus

A `Menu` is a server-side GUI: a chest, hopper, dispenser or any other vanilla
container screen whose slots you fill with `ItemStack`s. Menus are read-only by
default — the client cannot take, move or drop anything, every click is
cancelled and reported to you — which is what a minigame lobby, a shop or a
kit selector needs. Opt into `editable()` to let the player move items between
the menu and their inventory.

## Building a menu

```rust
use voidmc::{ItemStack, Menu, MenuSlot, MenuType};

let menu = Menu::chest(3, "Kit selector")
    .slot(0, ItemStack::of("minecraft:diamond_sword", 1).unwrap())
    .slot(1, ItemStack::of("minecraft:bow", 1).unwrap())
    .on_click(|click| {
        if let MenuSlot::Menu(index) = click.slot {
            click.reply(&format!("You picked kit {index}"));
            click.close();
        }
    });

let hopper = Menu::hopper("Filter");
let any = Menu::new(MenuType::Anvil, "Rename");
```

| Constructor | Screen |
|---|---|
| `Menu::chest(rows, title)` | `generic_9x1` .. `generic_9x6` (rows clamped to 1..=6) |
| `Menu::hopper(title)` | `hopper` (5 slots) |
| `Menu::dispenser(title)` | `generic_3x3` (9 slots) |
| `Menu::new(MenuType::X, title)` | any `minecraft:menu` entry: anvil, beacon, furnaces, brewing stand, crafting, enchantment, grindstone, lectern, loom, merchant, shulker box, smithing, cartography table, stonecutter, crafter |

`MenuType::slot_count()` gives the number of slots the screen owns; the
player's main inventory and hotbar follow them in the window (except for the
lectern). Registry ids come from `voidmc_data`, never from constants.

Builder methods: `slot(i, stack)`, `fill(stack)`, `on_click(handler)`,
`editable()`. Out-of-range slot indices are ignored.

## Opening and closing

Use the `Menus` system param from a system or observer, or `WorldMenus` when
you hold `&mut World` (command handlers, item behaviours, click handlers):

```rust
use voidmc::{Menu, Menus, WorldMenus};

fn open_shop(mut menus: Menus, player: Entity) {
    menus.open(player, Menu::chest(1, "Shop"));
    // later
    menus.close(player);
}

fn from_command(ctx: &mut CommandContext) {
    let player = ctx.entity;
    ctx.with_world_mut(|world| WorldMenus::new(world).open(player, Menu::chest(1, "Shop")));
}
```

Opening inserts an `OpenMenu` component on the player. The `InventorySync`
phase allocates a container id (1..=100 per player, never 0), sends Open
Screen and the full contents, and from then on diffs the menu each tick.
Opening another menu closes the current one first, like vanilla.

To change an open menu, mutate the component — `OpenMenu` derefs to `Menu`:

```rust
fn tick_timer(mut open: Query<&mut OpenMenu>) {
    for mut menu in open.iter_mut() {
        menu.set_slot(8, ItemStack::of("minecraft:clock", 1).unwrap());
        menu.set_title("Round 2");
    }
}
```

Only the slots that changed are sent (`SetContainerSlot`). A title change
re-sends Open Screen with the same container id followed by the full
contents: the client rebuilds the screen on Open Screen, so the menu stays
open but must be refilled.

`menus.close(player)` / `WorldMenus::close`, removing `OpenMenu`, the player
pressing escape, opening another menu and disconnecting all end a menu. Each
fires a `MenuClosedEvent { player, container_id, reason }` with
`MenuCloseReason::Server | Client | Replaced | Disconnect`. When a menu closes,
anything left on the cursor is returned to the inventory (or dropped if it does
not fit) and the player window is resent.

## Clicks

Every Container Click on an open menu becomes a `MenuClickEvent`:

| Field | Meaning |
|---|---|
| `player` | the clicking player |
| `container_id` | the window the click landed in |
| `slot` | `MenuSlot::Menu(i)`, `MenuSlot::Inventory(i)` (an `Inventory` index, 9..=44) or `MenuSlot::Outside` |
| `button` | mouse button / hotbar key, as the client sent it |
| `input` | `ContainerInput::Pickup`, `QuickMove`, `Swap`, `Clone`, `Throw`, `QuickCraft`, `PickupAll` |
| `item` | what the clicked slot held before the click |

The event is fired from the `MenuClickDrain` phase in `Update`, after which
the menu's `on_click` handler runs with a `MenuClickContext`: the same fields
plus `reply`, `close`, `open(menu)`, `menu()` / `menu_mut()`, `players()` and
`with_world` / `with_world_mut`. Both see the same world state.

```rust
use voidmc::{MenuClickEvent, MenuSlot, On};

fn log_clicks(event: On<MenuClickEvent>) {
    if let MenuSlot::Menu(i) = event.slot {
        tracing::info!(player = ?event.player, slot = i, item = ?event.item, "menu click");
    }
}
```

### Read-only menus

The server never applies the click. The slots the client believed it changed
(and the cursor) are resent with their true contents, so the screen snaps back
before the handler even runs. A click whose state id is stale gets a full
content resync, as in vanilla.

### Editable menus

With `editable()` the click runs through the same server-authoritative engine
as the player inventory (pickup, split, merge, shift-move, hotbar swap, drag,
double-click, throw), across the menu slots and the player's main inventory
and hotbar. Only the slots that actually changed are resent. Slot indices
outside the window are ignored; thrown items become `ItemDropEvent`s. The
click event still fires, so you can react after the fact (validate a deposit,
close the menu, etc.).

## Example

`void-example` registers `/menu`, a one-row chest whose clicks send a chat
message and whose last slot closes the menu (`void-example/src/menu.rs`).
