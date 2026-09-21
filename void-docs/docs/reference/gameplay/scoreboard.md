# Scoreboard

Objectives and teams follow the [boss bar](boss-bars.md) pattern: each is an
entity carrying an `Objective` or `Team` component. Spawn it to show it, mutate
it to update it, despawn it to remove it. The framework tracks what every
viewer has been sent and pushes only the difference in `PostUpdate`
(`VoidSystems::ScoreboardSync`): one `Set Score` per changed score, a
`Reset Score` per removed one, an objective or team update only when a header
field changed, and entity add/remove lists for team membership. Nothing is
sent on ticks where nothing changed.

## Objectives

```rust
use voidmc::{Objective, ScoreFormat, TextColor};

fn start_race(mut commands: Commands) {
    commands.spawn(
        Objective::sidebar("race")
            .title("Alpine Rush")
            .color(TextColor::Gold)
            .score("Leo", 3)
            .score("Adam", 1),
    );
}

fn lap_done(mut objectives: Query<&mut Objective, With<Race>>, name: &str) {
    for mut objective in &mut objectives {
        let laps = objective.get(name).unwrap_or(0);
        objective.set(name, laps + 1);
    }
}
```

| Field | Type | Default |
|---|---|---|
| `name` | `String` | required, unique per client |
| `slot` | `DisplaySlot` (`Sidebar`, `List`, `BelowName`, `SidebarTeam*`) | from the constructor |
| `title` | `String` | the name |
| `color` | [`TextColor`](messages.md#colours) | `White` |
| `render` | `RenderType` (`Integer`, `Hearts`) | `Integer` |
| `format` | `Option<ScoreFormat>` | `None` (plain red numbers) |
| `scores` | `BTreeMap<String, Score>` | empty |
| `audience` | [`Audience`](../server/sending-packets.md#audiences) | `Audience::All` |

Constructors: `Objective::sidebar(name)`, `Objective::list(name)`,
`Objective::below_name(name)`, `Objective::new(name, slot)`. Builder methods
(`.title()`, `.color()`, `.render()`, `.hearts()`, `.format()`,
`.score(owner, value)`, `.audience()`, `.viewers([..])`) set the same fields.

Scores are keyed by owner (a player name, or any string for sidebar lines).
`set(owner, value)` inserts or updates, `get(owner)` reads, `remove(owner)`
resets the line on every client, `clear()` resets them all. Each `Score`
also carries an optional `display` text shown instead of the owner and an
optional per-line `format`; reach them through `score_mut(owner)` or the
`scores` map directly.

`ScoreFormat` decides how the number renders: `Blank` hides it (classic
text-only sidebars), `Styled(TextColor)` colours it, `Fixed(text)` replaces it
with fixed text. Set it on the objective for every line or on one `Score` to
override a single line.

Changing `slot` moves the objective (the old slot is cleared); changing `name`
removes the old objective from every viewer and creates the new one. Titles and
texts are cut to the NBT string limit the same way as
[messages](messages.md#text-length).

## Teams

```rust
use voidmc::{CollisionRule, NameTagVisibility, Team, TextColor};

fn make_teams(mut commands: Commands, red_players: Vec<Entity>) {
    commands.spawn(
        Team::new("red")
            .color(TextColor::Red)
            .prefix("[R] ")
            .collision(CollisionRule::Never)
            .name_tags(NameTagVisibility::HideForOtherTeams)
            .members(red_players),
    );
}

fn join(mut teams: Query<&mut Team>, player: Entity) {
    for mut team in &mut teams {
        team.remove(player);
        if team.name == "red" {
            team.add(player);
        }
    }
}
```

| Field | Type | Default |
|---|---|---|
| `name` | `String` | required, unique per client |
| `display_name` | `String` | the name |
| `color` | `TeamColor` (16 named colours, `Bold`.., `Reset`) | `Reset` |
| `prefix`, `suffix` | `String` | empty |
| `friendly_fire` | `bool` | `true` |
| `see_invisible_friends` | `bool` | `true` |
| `name_tags` | `NameTagVisibility` (`Always`, `Never`, `HideForOtherTeams`, `HideForOwnTeam`) | `Always` |
| `collision` | `CollisionRule` (`Always`, `Never`, `PushOtherTeams`, `PushOwnTeam`) | `Always` |
| `players` | `HashSet<Entity>` | empty |
| `entries` | `BTreeSet<String>` | empty |
| `audience` | [`Audience`](../server/sending-packets.md#audiences) | `Audience::All` |

`.color()` takes a `TeamColor` or a `TextColor`; an RGB `TextColor` has no
team equivalent and falls back to `Reset` with a warning. The team colour
applies to the whole name line (prefix, name and suffix).

Members come in two forms. `add(entity)` / `remove(entity)` take player
entities and resolve their `PlayerName` when syncing; a player that disconnects
is dropped from every team automatically. `add_entry(text)` /
`remove_entry(text)` take raw scoreboard entries (entity UUIDs, or names of
players who are not online) and are sent verbatim.

## Audience and uniqueness

`audience` picks the viewers exactly like a boss bar: late joiners receive the
full state (objective, every score and the display slot; team parameters and
every member), players leaving the audience get a remove packet, and a
narrowed or widened `Audience` diffs the viewer set on the next tick.

Objective and team names are per client, so two objectives may share a name as
long as no player sees both (one personal sidebar per player, all named
`"sidebar"`, is fine). When a second entity would show an already-used name to
a player, that player is skipped for the newcomer and a warning is logged once
for the entity; it takes over the name when the first entity is removed.

## Removing

Despawn the entity, or remove the `Objective` / `Team` component; viewers get
the remove packet immediately. The client drops an objective's scores and
display slot together with it.

The example server shows a live altitude sidebar and a `/team <name> [--leave]`
command (`void-example/src/scoreboard.rs`).
