# Sidebar

`Sidebar` is the high-level face of the [scoreboard](scoreboard.md): a title
with an optional ornament and a list of **widgets** rendered into the sidebar
slot. It is an entity component like a [boss bar](boss-bars.md): spawn it to
show it, mutate its widgets to update it, despawn it to remove it. Every
change is rendered in `PostUpdate` (`VoidSystems::SidebarSync`) into the
entity's managed `Objective`, and the scoreboard sync then sends only the
lines whose text actually changed. Nothing is sent on quiet ticks.

```rust
use voidmc::{Audience, Ornament, Sidebar, TextColor, Widget, WidgetId};

#[derive(Component)]
struct RaceBoard {
    tour: WidgetId,
    ranking: WidgetId,
}

fn start_race(mut commands: Commands) {
    let mut sidebar = Sidebar::new("Alpine Rush")
        .color(TextColor::Gold)
        .ornament(Ornament::brackets("✦"))
        .audience(Audience::All);
    let tour = sidebar.push(Widget::labeled("Tour", "1/3"));
    let ranking = sidebar.push(Widget::ranking().limit(5));
    sidebar.push(Widget::blank());
    sidebar.push(Widget::labeled("Meilleur tour", "--:--"));
    commands.spawn((sidebar, RaceBoard { tour, ranking }));
}

fn lap_done(mut boards: Query<(&mut Sidebar, &RaceBoard)>, standings: Vec<(String, i32)>) {
    for (mut sidebar, board) in &mut boards {
        sidebar.set_value(board.tour, "2/3");
        if let Some(ranking) = sidebar.ranking_mut(board.ranking) {
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
| `audience` | [`Audience`](../server/sending-packets.md#audiences) | `Audience::All` |

Builder methods `.title()`, `.color()`, `.ornament()`, `.audience()`,
`.viewers([..])` set the same fields; `.widget(..)` appends a widget (any
widget or widget builder) when its handle is not needed. The widget list is
private and reached through handles (below).

## Title

The title is one coloured text; the ornament wraps it: `Ornament::brackets("◆")`
renders `◆ Alpine Rush ◆`, `Ornament::Line` renders `─ Alpine Rush ─`,
`Ornament::None` leaves it alone. `set_title(..)` or assigning `title`,
`color` or `ornament` sends a single objective update.

## Widgets

Each widget renders one or more lines. `Widget::xxx(..)` returns a builder for
the widgets that have options; every builder converts into a `Widget`, so it
can be passed to `.widget(..)`, `push`, `insert_before` / `insert_after` or
`set` directly.

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

Widgets are addressed by a `WidgetId`, the opaque handle `push` returns. A
handle stays valid for the life of its widget whatever is inserted or removed
around it, so keep the handles of the widgets you update in a component next
to the sidebar (as `RaceBoard` above) rather than positions. Every method
taking a handle is fallible in one way: it returns `false` / `None` when the
handle is unknown to this sidebar (or, for the typed accessors, when the
widget is of another kind), never panics. Handles are minted per sidebar, so a
handle from another board is not rejected on its own: keep each board's handles
with that board. The only panicking access is
`sidebar[id]` / `&mut sidebar[id]`, which behaves like `Vec` indexing.

| Method | Effect |
|---|---|
| `push(widget) -> WidgetId` | append a widget and return its handle |
| `insert_before(id, widget)` / `insert_after(id, widget)` `-> Option<WidgetId>` | insert next to an existing widget; `None` if `id` is unknown |
| `set(id, widget) -> bool` | replace a widget, keeping its handle |
| `remove(id) -> Option<Widget>` | remove a widget; its handle is never reused |
| `set_value(id, text) -> bool` | new value of a `Labeled` (or text of a `Text`); `false` if the widget is something else |
| `labeled_mut(id)`, `ranking_mut(id)`, `timer_mut(id)`, `progress_mut(id)` | `Option<&mut ..>` typed access: `ranking.set(entries)`, `ranking.set_highlight(Some("Leo"))`, `timer.set(Duration)` / `timer.set_ticks(t)`, `progress.set(v)` / `progress.set_max(m)`, `labeled.set_value(..)` |
| `get(id)` / `get_mut(id)` / `sidebar[id]` | the raw `Widget` |
| `widgets()` / `ids()` / `len()` / `is_empty()` | iterate `(WidgetId, &Widget)` pairs or handles in display order |
| `truncate(len)` / `clear()` | drop the widgets past `len`, or all of them |
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
characters (colour codes do not count, and a code left dangling at the end
of a line is dropped); the wire has no limit but longer lines only stretch the
panel. Lines are keyed by position (`line00`..`line14`) with descending scores and a
`Blank` number format, so no numbers show; changing one widget resends its
line only, while inserting or removing a widget resends every line below it.

## Audience

`audience` behaves as for a boss bar: late joiners get the whole board,
players leaving the audience get the remove packet. One sidebar per player:
the slot is per client, so two sidebars with disjoint audiences coexist (a
personal board per player is simply one `Sidebar` per player with
`.viewers([player])`), while a second sidebar shown to a player who already
has one is refused for that player with a warning until the first goes away
(see [Audience and uniqueness](scoreboard.md#audience-and-uniqueness)). The
first board to reach a player keeps them; a later one does not take over.

Audiences must therefore not overlap. A global board plus a personal
"highlight me" board for some players is written as one board per player, or
as a global board whose audience excludes the players who have their own:

```rust
let personal: Arc<RwLock<HashSet<Entity>>> = ..;
let shared = personal.clone();
commands.spawn(
    Sidebar::new("Alpine Rush")
        .audience(Audience::custom(move |r| !shared.read().unwrap().contains(&r.entity()))),
);
commands.spawn(Sidebar::new("Alpine Rush").viewers([leo]));
```

The custom audience is re-evaluated every tick: adding a player to `personal`
removes the global board from them on the next tick, and their own board
(spawned earlier and refused so far) takes the freed slot the tick after.

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
overwritten from the sidebar on every change, and an objective found on another
display slot is moved to the sidebar slot with a warning: do not edit them
directly, and read the viewers from `ObjectiveState::viewers()`. Despawning the
entity or removing the `Sidebar` component removes the board (the `Objective`
goes with it); the objective layer handles the packets and releases the
per-viewer name and slot.

The example server shows an altitude ranking with the player count and the
server uptime (`void-example/src/sidebar.rs`).
