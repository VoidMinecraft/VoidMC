# Messages

Messages are fire-and-forget requests, like [particles](particles.md) and
[sounds](sounds.md): start one from `Messages` (a `SystemParam`) or
`WorldMessages::new(&world)` (from command handlers, item behaviours and other
`&World` code), chain the settings you need, and finish with `.send()`. They
are the text-messaging entry point of the framework — the `SystemChat` packet
(text component + `overlay` flag) is built for you, so call sites never touch
NBT. A request that is never sent does nothing (the builder is `#[must_use]`).

```rust
use voidmc::{Audience, DimensionId, Messages, WorldMessages};

fn on_join(event: On<PlayerReadyEvent>, messages: Messages) {
    messages.message(event.entity, "Welcome!").color("green").send();
    messages.broadcast(format!("{} joined", event.name)).color("yellow").send();
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
| `color(name)` | `"white"` | A named colour (`"red"`, `"gray"`, `"gold"`, …) or `"#rrggbb"`. |
| `audience(Audience)` / `viewers(entities)` | see above | Replace the target with the ready players the audience selects. |
| `send()` | — | Consumes the request and sends it. |

`packet()` returns the `SystemChat` that `send()` would send, for tests.

`CommandContext::reply`, `reply_error` and `broadcast` are shorthands over
`WorldMessages`. For a packet that is not a system message, use
[`Players`](../server/sending-packets.md) directly.
