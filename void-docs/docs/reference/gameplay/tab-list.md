# Tab List

The tab list has two customisable parts, both stateful and both synced by the
framework with push-on-change diffs:

- the **header and footer** shown above and below the player rows — a
  `TabList` entity with an [`Audience`](../server/sending-packets.md#audiences);
- each **player row** — a `TabEntry` component on the player entity, seen by
  every ready player.

The join and quit flow (Player Info Update `ADD_PLAYER`, Player Info Remove)
is handled by the engine; you only ever mutate components.

## Header and footer

```rust
use voidmc::{TabList, TextColor};

fn on_start(mut commands: Commands) {
    commands.spawn(
        TabList::new()
            .header("Alpine Rush")
            .header_color(TextColor::Gold)
            .footer("void.mc"),
    );
}
```

| Field | Type | Default |
|---|---|---|
| `header` | `String` | `""` |
| `footer` | `String` | `""` |
| `header_color` / `footer_color` | [`TextColor`](messages.md#colours) | `White` |
| `audience` | [`Audience`](../server/sending-packets.md#audiences) | `Audience::All` |

Mutate the component to update it: a changed header, footer or colour sends
one Set Tab List Header And Footer packet to the current viewers in the next
`PostUpdate` (`VoidSystems::TabListSync`). Players who join the audience later
receive the current text on their first ready tick; players who leave it, and
every viewer when the entity is despawned or the component removed, get an
empty header and footer. Multi-line text uses `\n`. Texts are cut to the NBT
string limit like [messages](messages.md#text-length).

A client shows exactly one header and footer, so give overlapping audiences
to several `TabList` entities only on purpose: the last one synced in a tick
wins.

## Player rows

Every ready player carries a `TabEntry` (the engine inserts the default one
when the player becomes ready if you did not already). Mutate it, or insert
your own before or after the player is ready:

```rust
use voidmc::{TabEntry, TextColor};

fn crown(mut entries: Query<&mut TabEntry>, winner: Entity) {
    if let Ok(mut entry) = entries.get_mut(winner) {
        entry.display_name = Some("★ Leo".into());
        entry.color = TextColor::Gold;
        entry.list_order = 10;
    }
}

fn vanish(mut commands: Commands, player: Entity) {
    commands.entity(player).insert(TabEntry::new().hidden());
}
```

| Field | Type | Default | Wire action |
|---|---|---|---|
| `display_name` | `Option<String>` | `None` (real name) | `UPDATE_DISPLAY_NAME` |
| `color` | `TextColor` | `White` | part of the display name |
| `listed` | `bool` | `true` | `UPDATE_LISTED` |
| `latency` | `Option<i32>` (ms) | `None` (measured) | `UPDATE_LATENCY` |
| `game_mode` | `Option<u8>` | `None` (server default) | `UPDATE_GAME_MODE` |
| `list_order` | `i32` | `0` | `UPDATE_LIST_ORDER` |
| `show_hat` | `bool` | `true` | `UPDATE_HAT` |

The builder methods (`.display_name()`, `.color()`, `.listed()`, `.hidden()`,
`.latency()`, `.game_mode()`, `.list_order()`, `.show_hat()`) set the same
fields. Each change becomes one Player Info Update packet carrying only the
changed actions, sent to every ready player. Rows changed in the same tick that
share the same set of actions travel in one packet. To revert a row, mutate the
component back to `TabEntry::new()` rather than removing it: removal also
reverts the row, but the player then no longer matches `Query<&mut TabEntry>`
until you insert a new one.

`latency: None` shows the keep-alive round trip the engine measures
(`KeepAliveState::latency`, refreshed every keep-alive, about every ten
seconds) and pushes an `UPDATE_LATENCY` only when the value changes. `/gamemode`
updates `game_mode` so the row's icon follows the player.

A player joining receives the full list built from what the other players were
last sent; any change that has not been synced yet reaches the newcomer with
the same diff as everyone else.

The example server registers `/tablist [header] [footer...]`
(`--clear`) and `/nick [name]` (`--hide`, `--reset`) as minimal demonstrations
(`void-example/src/tab_list.rs`).
