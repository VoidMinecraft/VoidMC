# Vanilla /team & /scoreboard

`voidmc-vanilla-commands` is an optional crate with vanilla-compatible
`/team` and `/scoreboard` commands. Nothing is built into the engine: add the
plugin to get both, with vanilla syntax, client-side tab completion and the
same success and error messages.

```rust
use voidmc_vanilla_commands::{Access, VanillaCommandsPlugin};

VoidServer::new(config)
    .add_plugin(|app| {
        app.add_plugins(VanillaCommandsPlugin::default());
    })
    .run();
```

| Plugin | Registers |
|---|---|
| `VanillaCommandsPlugin` | both commands |
| `TeamCommandsPlugin` | `/team` |
| `ScoreboardCommandsPlugin` | `/scoreboard` and the `Scoreboard` resource |

Each plugin has one field, `access`. `Access::Operators` (the default, as in
vanilla) only lets players carrying the `Operator` component run the command;
`Access::Everyone` opens it to all players, which the example server does.
The check is a [`requires`](commands.md#requirements): it runs before any
argument is parsed, so other players get no completions and no error that
names a team or objective. The engine sends every player the same command
tree, so they still see that both commands exist.
`team_command(access)` and `scoreboard_command(access)` return the bare
`Command` for a registry of your own; without `ScoreboardCommandsPlugin` the
scoreboard is still kept, but nothing is shown to clients.

## /team

```
/team add <team> [<displayName>]
/team remove <team>
/team empty <team>
/team join <team> [<members>]
/team leave <members>
/team list [<team>]
/team modify <team> displayName <displayName>
/team modify <team> color <color>
/team modify <team> friendlyFire <true|false>
/team modify <team> seeFriendlyInvisibles <true|false>
/team modify <team> nametagVisibility never|hideForOtherTeams|hideForOwnTeam|always
/team modify <team> deathMessageVisibility never|hideForOtherTeams|hideForOwnTeam|always
/team modify <team> collisionRule always|never|pushOtherTeams|pushOwnTeam
/team modify <team> prefix <prefix>
/team modify <team> suffix <suffix>
```

Teams are the engine's [`Team`](scoreboard.md#teams) entities: `/team add`
spawns one, `/team remove` despawns it, `/team modify` edits its fields, and
the scoreboard sync sends the difference. Teams spawned by code show up in
`/team list` and can be modified the same way; removing one only takes the
`Team` component off its entity.

Members join by name (`Team::entries`), as vanilla score holders do, so a
player keeps their team across reconnects and offline names or UUIDs can join
too. Online players are also kept in `Team::members`: when they join, and
again each time a player whose name a team lists becomes ready, so
`team.contains(player)` holds across reconnects and for players added while
offline. `join` and `leave` first take the holder off every other team, including
one that listed the player's entity in `Team::members`. `join` without members
adds the executor.

`deathMessageVisibility` is stored as a `DeathMessageVisibility` component on
the team entity for game code to read; the engine sends no death messages.

## /scoreboard

```
/scoreboard objectives list
/scoreboard objectives add <objective> <criteria> [<displayName>]
/scoreboard objectives remove <objective>
/scoreboard objectives setdisplay <slot> [<objective>]
/scoreboard objectives modify <objective> displayname <displayName>
/scoreboard objectives modify <objective> rendertype hearts|integer
/scoreboard objectives modify <objective> displayautoupdate <true|false>
/scoreboard objectives modify <objective> numberformat [blank|fixed <contents>|styled <style>]
/scoreboard players list [<target>]
/scoreboard players get <target> <objective>
/scoreboard players set <targets> <objective> <score>
/scoreboard players add <targets> <objective> <score>
/scoreboard players remove <targets> <objective> <score>
/scoreboard players reset <targets> [<objective>]
/scoreboard players enable <targets> <objective>
/scoreboard players operation <targets> <targetObjective> <operation> <source> <sourceObjective>
/scoreboard players display name <targets> <objective> [<text>]
/scoreboard players display numberformat <targets> <objective> [blank|fixed <contents>|styled <style>]
```

Vanilla objectives exist whether or not a slot shows them, so they live in the
`Scoreboard` resource rather than in entities. Whenever it changes, a
`PostUpdate` system (before `VoidSystems::ScoreboardSync`) mirrors every
display slot into an engine [`Objective`](scoreboard.md#objectives) entity
tagged `DisplayedObjective(slot)`, and the engine sends only what changed. Like
vanilla, an objective in no slot is never sent to clients.

Operations follow Java integer semantics: additions wrap, `/=` and `%=` round
towards negative infinity, and missing scores count as 0. `><` swaps. Dividing
by zero fails without changing any score (vanilla keeps the pairs it already
applied).

Display slots and objective names are shared with every other engine
`Objective`, such as a [`Sidebar`](sidebar.md). When one already holds the slot
or name for a player, that player keeps it, and `setdisplay` adds a warning to
its reply.

### From code

The resource is public, so game code and commands share the same scores:

```rust
use voidmc_vanilla_commands::{Criteria, Scoreboard};

fn setup(mut board: ResMut<Scoreboard>) {
    board.add_objective("laps", Criteria::Dummy).unwrap().title = "Laps".into();
    board.set_display(DisplaySlot::Sidebar, Some("laps"));
}

fn lap(mut board: ResMut<Scoreboard>, player: &str) {
    let laps = board.get(player, "laps").unwrap_or(0);
    board.set(player, "laps", laps + 1);
}
```

| Method | Does |
|---|---|
| `add_objective(name, criteria)` | `Some(&mut ScoreObjective)` to fill in, `None` if the name is taken |
| `remove_objective(name)` | removes it and clears the slots showing it |
| `objective(name)` / `objective_mut(name)` / `objectives()` | read or edit title, colour, render type, number format, scores |
| `get(holder, objective)` / `set(holder, objective, value)` | one score |
| `reset(holder, Some(objective) \| None)` | one score, or all of the holder's |
| `holders()` | every holder with a score (what `*` selects) |
| `display(slot)` / `set_display(slot, Some(name) \| None)` | display slots |

## Argument types

| Parser | `ctx.get::<T>` | Client parser | Accepts |
|---|---|---|---|
| `TeamArg` | `Entity` | `minecraft:team` + `ask_server` | An existing team name |
| `ObjectiveArg` | `String` | `minecraft:objective` + `ask_server` | An existing objective name |
| `NameArg` | `String` | `brigadier:string` (word) | A new name: `0-9 A-Z a-z _ - . +` |
| `CriteriaArg` | `Criteria` | `minecraft:objective_criteria` | `dummy` or `trigger` |
| `ScoreHolderArg::multiple()` / `::single()` | `Vec<String>` / `String` | `minecraft:score_holder` + `ask_server` | `*`, `@a`, `@s`, `@p`, `@r`, any name or UUID (an online player's UUID becomes their name) |
| `OperationArg` | `Operation` | `minecraft:operation` | `= += -= *= /= %= < > ><` |
| `SlotArg` | `DisplaySlot` | `minecraft:scoreboard_slot` | `list`, `sidebar`, `below_name`, `sidebar.team.<color>` |
| `ComponentArg` | `StyledText` | `minecraft:component` | `"text"`, `'text'`, a bare word, `{"text":..,"color":..}` (JSON or SNBT), or a list of those |
| `StyleArg` | `ScoreFormat` | `minecraft:style` | `{"color":"gold"}` |

`ComponentArg` and `StyleArg` take the rest of the line (nested at most 512
deep, like vanilla), so register them as
the last, variadic argument.

## Not supported

- Criteria other than `dummy` and `trigger` (`health`, `food`, `deathCount`,
  statistics, `teamkill.*`, ...): the engine tracks none of them, so
  `objectives add` rejects them rather than create a score that never moves.
- The `/trigger` command. Trigger objectives and `players enable` work, so a
  later `/trigger` can build on them.
- Entity selectors other than players (`@e`, `@n`) and selector arguments
  (`@a[team=red]`): the engine selectors cover `@s`, `@p`, `@a`, `@r` and
  names.
- Text styling beyond plain text: team display names, prefixes and suffixes are
  plain text (the engine's `Team` sends them uncoloured), objective display
  names keep only the root colour, and `styled` number formats only the
  colour. Translations and other component kinds render their raw key.
- `displayautoupdate` is stored but changes nothing: score holders' display
  names never change on their own.
- An objective shown in several slots is sent once per slot (the extra copies
  are named `<objective>#<slot id>` on the wire), so its score updates go out
  once per slot.
- Several spaces in a row inside a quoted component collapse into one, because
  command lines are split on whitespace before parsing.
