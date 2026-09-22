# Titles

Titles are fire-and-forget requests, like [messages](messages.md),
[particles](particles.md) and [sounds](sounds.md): start one from `Titles` (a
`SystemParam`) or `WorldTitles::new(&world)` (from command handlers, item
behaviours and other `&World` code), chain the settings you need, and finish
with `.send()`. The `SetTitlesAnimation`, `SetSubtitleText`, `SetTitleText` and
`ClearTitles` packets are built for you, so call sites never touch NBT. A
request that is never sent does nothing (the builder is `#[must_use]`).

```rust
use voidmc::{Audience, DimensionId, TextColor, Titles, WorldTitles};

fn start_round(titles: Titles, round: Res<Round>) {
    titles
        .broadcast(format!("Round {}", round.number))
        .subtitle("Get ready")
        .color(TextColor::Gold)
        .subtitle_color(TextColor::Gray)
        .times(10, 70, 20)
        .audience(Audience::InDimension(DimensionId::Overworld))
        .send();
}

fn on_finish(titles: Titles, winner: Entity) {
    titles.title(winner, "Winner!").times(0, 40, 10).send();
    titles.broadcast("Finish!").except(winner).send();
}

fn handle(ctx: &mut CommandContext) {
    ctx.with_world(|world| {
        WorldTitles::new(world).clear(ctx.entity).send();
    });
}
```

The `player` in `title` / `subtitle` / `clear` / `reset` is the *default*
target. Calling `.audience(..)`, `.viewers(..)` or `.except(..)` on it
replaces that target entirely: the player is dropped and the packets go to the
ready players the audience selects. `title(joining, "x").send()` reaches a
client that is not ready yet; `title(joining, "x").viewers([joining]).send()`
sends nothing.

## Starting a request

| Method | Sends | Default target |
|---|---|---|
| `title(player, text)` | title (+ subtitle / times if set) | That client entity — ready or not, like `Players::send`. |
| `subtitle(player, text)` | subtitle (+ title / times if set) | That client entity. |
| `broadcast(text)` | title (+ subtitle / times if set) | Every ready player (`Audience::All`). |
| `clear(player)` | `ClearTitles` keeping the times | That client entity. |
| `reset(player)` | `ClearTitles` restoring the default times | That client entity. |

`text` is `impl Into<String>` (`&str`, `String`, `format!(..)`).

## Request settings

| Method | Default | Meaning |
|---|---|---|
| `title(text)` / `subtitle(text)` | none | Sets (or replaces) that line. |
| `color(TextColor)` | `TextColor::White` | Colour of the title, see [messages](messages.md#colours). |
| `subtitle_color(TextColor)` | `TextColor::White` | Colour of the subtitle. |
| `times(fade_in, stay, fade_out)` | not sent | Animation in ticks. Without it the client keeps the times it last received (10 / 70 / 20 after `reset`). |
| `audience(Audience)` / `viewers(entities)` | see above | **Replaces** the target: delivery becomes ready-only, exactly as for `broadcast`. |
| `except(entity)` | — | Narrows the current audience (`Audience::All` for a single-player request) so `entity` is skipped. |
| `send()` | — | Consumes the request and sends it. |

`packets()` returns the packets `send()` would send, in wire order, for tests.
`ClearTitlesRequest` has the same `audience` / `viewers` / `except` / `send`, and
`packet()` returns its `ClearTitles`.

## Packet order

`send()` encodes each line once and emits, per recipient, `SetTitlesAnimation`
(if `times` was set), then `SetSubtitleText` (if a subtitle was set), then
`SetTitleText` (if a title was set) — the vanilla order, because the client
starts the animation when the title arrives. A subtitle-only request is valid
and only updates the stored subtitle: the client shows a subtitle only while a
title is on screen, so send the title afterwards (or in the same request).

The reverse also holds: **a request without `subtitle` leaves the previous
subtitle on screen.** The client keeps the last `SetSubtitleText` it received
until `ClearTitles` or a new subtitle arrives, so `title(p, "Round 2")
.subtitle("Get ready").send()` followed later by `title(p, "GO!").send()` shows
"GO!" over "Get ready". Send `.subtitle("")` with the new title (or `clear`
first) to drop it:

```rust
titles.title(player, "GO!").subtitle("").send();
```

`clear` hides the current title and subtitle but keeps the animation times;
`reset` also restores the default times (10 / 70 / 20).

## Text length

Title and subtitle text goes through the same guard as
[messages](messages.md#text-length): anything past the NBT string limit is cut
on a character boundary with a warning instead of kicking the recipient.
