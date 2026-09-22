# Messages

Messages are fire-and-forget requests, like [particles](particles.md) and
[sounds](sounds.md): start one from `Messages` (a `SystemParam`) or
`WorldMessages::new(&world)` (from command handlers, item behaviours and other
`&World` code), chain the settings you need, and finish with `.send()`. They
are the text-messaging entry point of the framework — the `SystemChat` packet
(text component + `overlay` flag) is built for you, so call sites never touch
NBT. A request that is never sent does nothing (the builder is `#[must_use]`).

```rust
use voidmc::components::PlayerName;
use voidmc::events::PlayerReadyEvent;
use voidmc::{Audience, DimensionId, Messages, On, Query, TextColor, WorldMessages};

fn on_join(event: On<PlayerReadyEvent>, messages: Messages, names: Query<&PlayerName>) {
    let name = names.get(event.entity).map(|n| n.0.as_str()).unwrap_or("Someone");
    messages
        .message(event.entity, "Welcome!")
        .color(TextColor::Green)
        .send();
    messages
        .broadcast(format!("{name} joined"))
        .color(TextColor::Yellow)
        .send();
}

fn tick_timer(messages: Messages, timer: Res<RoundTimer>) {
    messages
        .broadcast_action_bar(format!("{}s left", timer.remaining))
        .audience(Audience::InDimension(DimensionId::Overworld))
        .send();
}

fn handle(ctx: &mut CommandContext) {
    ctx.with_world(|world| {
        WorldMessages::new(world).action_bar(ctx.entity, "Teleported").send();
    });
}
```

The `player` in `message` / `action_bar` is the *default* target. Calling
`.audience(..)` or `.viewers(..)` on it replaces that target entirely: the
player is dropped and the message goes to the ready players the audience
selects. `message(joining, "x").send()` reaches a client that is not ready
yet; `message(joining, "x").viewers([joining]).send()` sends nothing.

## Starting a request

| Method | Line | Default target |
|---|---|---|
| `message(player, text)` | chat | That client entity — ready or not, like `Players::send`. |
| `action_bar(player, text)` | action bar (overlay) | That client entity. |
| `broadcast(text)` | chat | Every ready player (`Audience::All`). |
| `broadcast_action_bar(text)` | action bar (overlay) | Every ready player. |

`text` is `impl Into<String>` (`&str`, `String`, `format!(..)`).

## Request settings

| Method | Default | Meaning |
|---|---|---|
| `color(TextColor)` | `TextColor::White` | One of the 16 vanilla colours or `TextColor::rgb(0xrrggbb)`. |
| `audience(Audience)` / `viewers(entities)` | see above | **Replaces** the target: the `player` given to `message`/`action_bar` is discarded and delivery becomes ready-only, exactly as for `broadcast`. |
| `except(entity)` | — | Narrows the current audience (`Audience::All` for a single-player request) so `entity` is skipped: `broadcast("x").except(sender)`. |
| `send()` | — | Consumes the request and sends it. |

`packet()` returns the `SystemChat` that `send()` would send, for tests.

## Colours

`TextColor` is the only way to colour a message, because the client rejects
anything that is not one of the 16 vanilla names (lowercase, exact) or
`#rrggbb` — a bad string does not fail on the server, it **disconnects the
recipient** with a `DecoderException`. The variants are `Black`, `DarkBlue`,
`DarkGreen`, `DarkAqua`, `DarkRed`, `DarkPurple`, `Gold`, `Gray`, `DarkGray`,
`Blue`, `Green`, `Aqua`, `Red`, `LightPurple`, `Yellow`, `White` and
`Rgb(u32)`; `TextColor::rgb(value)` masks to 24 bits and serialises as
`#rrggbb`. `TextColor::parse` accepts the 16 vanilla names and `#rrggbb`
(`parse("dark_red")` / `parse("#ff8800")`), returning `None` when the client
would reject the input, and `Display` gives the wire name back.

## Text length

Text longer than the NBT string limit (65 535 modified-UTF-8 bytes, where
`\0` costs 2 bytes and characters outside the BMP cost 6) is cut on a
character boundary and a warning is logged, rather than sent truncated
mid-character. `messages::text_component(text, color)` and
`messages::plain_text_component(text)` build a guarded component for any
other packet; menu titles, boss bar titles, custom names and text displays go
through the same guard.

The legacy string entry points `commands::system_chat(text, color)` and
`commands::text_to_nbt(text, color)` keep their signatures: an invalid colour
logs a `warn!` and falls back to white instead of kicking the client. Prefer
`Messages` / `WorldMessages` in new code.

`CommandContext::reply`, `reply_error` and `broadcast` are shorthands over
`WorldMessages`. On-screen titles and advancement popups have their own
request types, see [titles](titles.md) and [toasts](toasts.md). For a packet that is not a system message, use
[`Players`](../server/sending-packets.md) directly.
