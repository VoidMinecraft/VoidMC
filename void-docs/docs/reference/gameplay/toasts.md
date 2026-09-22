# Toasts

Toasts are the advancement popups in the top-right corner of the screen, used
here as fire-and-forget notifications like [messages](messages.md) and
[titles](titles.md): start one from `Toasts` (a `SystemParam`) or
`WorldToasts::new(&world)` (from command handlers, item behaviours and other
`&World` code), chain the settings you need, and finish with `.send()`. Each
request becomes a synthetic advancement that is granted and removed in two
back-to-back `UpdateAdvancements` packets, so the popup shows and nothing
lingers in the client's advancement screen. A request that is never sent does
nothing (the builder is `#[must_use]`).

```rust
use voidmc::{Audience, DimensionId, ItemId, Sound, TextColor, ToastFrame, Toasts, WorldToasts};

fn on_record(toasts: Toasts, player: Entity, time: String) {
    toasts
        .toast(player, "New record!")
        .description(time)
        .icon(ItemId::from_name("gold_ingot").unwrap())
        .frame(ToastFrame::Challenge)
        .color(TextColor::Gold)
        .send();
}

fn on_finish(toasts: Toasts, dimension: DimensionId) {
    toasts
        .broadcast("Race over")
        .icon(ItemId::from_name("minecart").unwrap())
        .sound(Sound::new("entity.player.levelup"))
        .audience(Audience::InDimension(dimension))
        .send();
}

fn handle(ctx: &mut CommandContext) {
    ctx.with_world(|world| {
        WorldToasts::new(world).toast(ctx.entity, "Hello").send();
    });
}
```

The `player` in `toast` is the *default* target. Calling `.audience(..)`,
`.viewers(..)` or `.except(..)` replaces that target entirely: the player is
dropped and the packets go to the ready players the audience selects.
`toast(joining, "x").send()` reaches a client that is not ready yet;
`toast(joining, "x").viewers([joining]).send()` sends nothing.

## Starting a request

| Method | Default target |
|---|---|
| `toast(player, title)` | That client entity — ready or not, like `Players::send`. |
| `broadcast(title)` | Every ready player (`Audience::All`). |

`title` is `impl Into<String>` (`&str`, `String`, `format!(..)`).

## Request settings

| Method | Default | Meaning |
|---|---|---|
| `color(TextColor)` | `TextColor::White` | Colour of the title, see [messages](messages.md#colours). |
| `description(text)` | `""` | Shown when hovering the popup; the popup itself only renders the title. |
| `description_color(TextColor)` | `TextColor::White` | Colour of the description. |
| `icon(item)` | `minecraft:paper` | An `ItemId` or a full `ItemStack` (components such as damage are honoured, the count is never displayed). An empty stack falls back to the default. |
| `frame(ToastFrame)` | `ToastFrame::Task` | `Task` (plain), `Goal` (rounded) or `Challenge` (spiky; the client also plays its own fanfare). |
| `sound(Sound)` | none | A [sound](sounds.md) played at each recipient's own position, unless it is placed with `Sound::at(..)`; the sound's audience is ignored. |
| `audience(Audience)` / `viewers(entities)` | see above | **Replaces** the target: delivery becomes ready-only, exactly as for `broadcast`. |
| `except(entity)` | — | Narrows the current audience (`Audience::All` for a single-player request) so `entity` is skipped. |
| `send()` | — | Consumes the request and sends it. |

`packets()` returns the `(add, remove)` pair `send()` would send, for tests.
Every call claims a fresh synthetic id (`void:toast/<n>`), so two toasts sent
in the same tick never collide on the client.

## Packet order

`send()` builds the pair once and emits, per recipient, the `UpdateAdvancements`
that adds the advancement (display with `show_toast`, criterion granted with the
current time), then the one that removes it, then the sound if any. The client
shows the popup on the first packet and forgets the advancement on the second.

## Text length

Title and description go through the same guard as
[messages](messages.md#text-length): anything past the NBT string limit is cut
on a character boundary with a warning instead of kicking the recipient.
