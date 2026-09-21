# Sidebar

`Sidebar` is the high-level face of the [scoreboard](scoreboard.md): a title
with an optional ornament and a list of **widgets** rendered into the sidebar
slot. It is an entity component like a [boss bar](boss-bars.md): spawn it to
show it, mutate its widgets to update it, despawn it to remove it. Every
change is rendered in `PostUpdate` (`VoidSystems::SidebarSync`) into the
entity's managed `Objective`, and the scoreboard sync then sends only the
lines whose text actually changed. Nothing is sent on quiet ticks.

```rust
use voidmc::{Audience, Ornament, Sidebar, TextColor, Widget};

fn start_race(mut commands: Commands) {
    commands.spawn(
        Sidebar::new("Alpine Rush")
            .color(TextColor::Gold)
            .ornament(Ornament::brackets("✦"))
            .widget(Widget::labeled("Tour", "1/3"))
            .widget(Widget::ranking().limit(5))
            .widget(Widget::blank())
            .widget(Widget::labeled("Meilleur tour", "--:--"))
            .audience(Audience::All),
    );
}

fn lap_done(mut sidebars: Query<&mut Sidebar, With<Race>>, standings: Vec<(String, i32)>) {
    for mut sidebar in &mut sidebars {
        sidebar.set_value(0, "2/3");
        if let Some(ranking) = sidebar.ranking_mut(1) {
            ranking.set(standings.clone());
        }
    }
}
```

| Field | Type | Default |
|---|---|---|
| `title` | `String` | required |
| `color` | [`TextColor`](messages.md#colours) | `White` |
| `ornament` | `Ornament` (`None`, `Brackets(glyph)`, `Line`) | `None` |
| `widgets` | `Vec<Widget>` | empty |
| `audience` | [`Audience`](../server/sending-packets.md#audiences) | `Audience::All` |

Builder methods `.title()`, `.color()`, `.ornament()`, `.widget(..)` (any
widget or widget builder), `.audience()`, `.viewers([..])` set the same fields.

## Title

The title is one coloured text; the ornament wraps it: `Ornament::brackets("◆")`
renders `◆ Alpine Rush ◆`, `Ornament::Line` renders `─ Alpine Rush ─`,
`Ornament::None` leaves it alone. `set_title(..)` or assigning `title`,
`color` or `ornament` sends a single objective update.

## Widgets

Each widget renders one or more lines. `Widget::xxx(..)` returns a builder for
the widgets that have options; every builder converts into a `Widget`, so it
can be passed to `.widget(..)`, `push`, `insert` or `set` directly.

| Widget | Builder | Renders |
|---|---|---|
| `Text` | `Widget::text("Bienvenue").color(c)` | the text, optionally coloured |
| `Labeled` | `Widget::labeled("Tour", "1/3").color(c)` | `Tour: 1/3`, the value in `c` (default white) |
| `Blank` | `Widget::blank()` | an empty line |
| `Separator` | `Widget::separator('─')` | 20 dark-gray copies of the glyph |
| `Ranking` | `Widget::ranking().limit(5).highlight("Leo").color(c).entries([("Leo", 1200)])` | one line per entry, best first: `1. Leo 1200`; the highlighted name in `c` (default yellow), at most `limit` lines (default 5) |
| `Timer` | `Widget::timer(Duration)` / `Widget::timer_ticks(ticks)` `.label("Temps").color(c)` | `mm:ss`, or `h:mm:ss` past an hour, optionally prefixed by `Temps: ` |
| `Progress` | `Widget::progress(value, max).width(10).label("HP").color(c)` | `▮▮▮▯▯▯▯▯▯▯`, filled part in `c` (default green) |

Colours on lines are the 16 named `TextColor`s; an RGB colour falls back to
white with a warning (score lines carry legacy colour codes, which have no
RGB form).

Mutate widgets through the sidebar:

| Method | Effect |
|---|---|
| `push(widget) -> usize` / `insert(index, widget)` / `set(index, widget)` / `remove(index)` / `clear()` | edit the widget list; `push` returns the new widget's index |
| `set_value(index, text) -> bool` | new value of a `Labeled` (or text of a `Text`); `false` if the widget is something else |
| `labeled_mut(i)`, `ranking_mut(i)`, `timer_mut(i)`, `progress_mut(i)` | `Option<&mut ..>` typed access: `ranking.set(entries)`, `ranking.set_highlight(Some("Leo"))`, `timer.set(Duration)` / `timer.set_ticks(t)`, `progress.set(v)` / `progress.set_max(m)`, `labeled.set_value(..)` |
| `get(i)` / `get_mut(i)` | the raw `Widget` |
| `render()` / `rendered_title()` | the lines and title as they will be sent, for tests |

Any mutation through `Mut<Sidebar>` marks the sidebar changed and re-renders
it that tick, even when the rendered lines end up identical (then nothing is
sent). A timer driven every tick therefore costs a render per tick but a
packet per second; compare before writing when the value rarely changes, as
the example server's `altitude_system` does.

## Lines and limits

The sidebar shows at most **15 lines** (vanilla limit): widgets past the
fifteenth line are rendered but not shown, and a warning is logged once when a
sidebar starts overflowing. Each line is cut to `MAX_LINE_CHARS` (40) visible
characters; the wire has no limit but longer lines only stretch the panel.
Lines are keyed by position (`line00`..`line14`) with descending scores and a
`Blank` number format, so no numbers show; changing one widget resends its
line only, while inserting or removing a widget resends every line below it.

## Audience

`audience` behaves as for a boss bar: late joiners get the whole board,
players leaving the audience get the remove packet. One sidebar per player:
the slot is per client, so two sidebars with disjoint audiences coexist (a
personal board per player is simply one `Sidebar` per player with
`.viewers([player])`), while a second sidebar shown to a player who already
has one is refused for that player with a warning until the first goes away
(see [Audience and uniqueness](scoreboard.md#audience-and-uniqueness)).

Header and footer are not sidebar lines: they belong to the tab list
(`TabList::new().header(..).footer(..)`). The three together:

```rust
commands.spawn(TabList::new().header("Alpine Rush").header_color(TextColor::Gold).footer("void.mc"));
commands.spawn(Sidebar::new("Alpine Rush").color(TextColor::Gold).ornament(Ornament::Line)
    .widget(Widget::timer(Duration::ZERO).label("Temps"))
    .widget(Widget::ranking().limit(5)));
commands.spawn(BossBar::new("Tour 1/3").progress(0.0));
```

## Under the hood

`Sidebar` requires a managed `Objective` (named `"sidebar"`, `Blank` format)
and a `SidebarState`. The objective's title, colour, scores and audience are
overwritten from the sidebar on every change: do not edit them directly, and
read the viewers from `ObjectiveState::viewers()`. Despawning the entity or
removing the `Sidebar` component removes the board; the objective layer
handles the packets.

The example server shows an altitude ranking with the player count and the
server uptime (`void-example/src/sidebar.rs`).
