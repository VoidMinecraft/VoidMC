# Command System

## Overview

Void includes a full command system with typed argument parsing, flag support, auto-generated usage strings, and client-side tab-completion via the Minecraft protocol command tree.

## Command Pipeline

This diagram traces the full lifecycle of a chat command — from the client pressing Enter to the handler running.

```
Client presses Enter
       │
       ▼
  TCP frame → void-net decodes bytes → raw Packet
       │
       ▼
  void/src/network.rs  ingest_network_packets  (PreUpdate system)
       │  decodes to serverbound::PlayPacket
       │  calls world.trigger(PacketEvent<T>)
       ▼
  void/src/plugins/chat.rs  observers
  ├── handle_chat_command        (ChatCommand 0x07)
  ├── handle_signed_chat_command (SignedChatCommand 0x08)
  └── handle_chat_message        (ChatMessage 0x09 starting with '/')
       │  all three call handle_command()
       ▼
  handle_command()
  ├── splits raw string → [command_name, args...]
  ├── enqueue_command() → pushes QueuedCommand to CommandQueue resource
  └── commands.trigger(ChatCommandEvent)  ← external observers hook here
       │
       ▼
  void/src/commands/plugin.rs  drain_command_queue  (Update system)
       │  pops each QueuedCommand in FIFO order
       ▼
  dispatch_command()  in  void/src/commands/mod.rs
  ├── resolves name/alias in CommandRegistry
  ├── flags::extract_flags() — peels off --flags / -f tokens, values parsed with parse_in()
  ├── parse_positional()     — calls ArgParser::parse_in() per argument
  │   │                        (executor context: `~`/`^` coordinates, `@s`/`@p` selectors)
  │   └── on error → sends red usage message, returns early
  └── calls handler(&mut CommandContext)
       │
       ▼
  Command handler  (e.g. handle_summon in commands/defaults.rs)
  └── ctx.get::<T>("arg_name"), ctx.reply(), ctx.with_world_mut(...)
```

### Why two packet types?

The client sends commands via two different packets:

- **`ChatCommand` (0x07)** — unsigned, sent for most commands.
- **`SignedChatCommand` (0x08)** — cryptographically signed when the client has chat signing enabled.

Both are routed through the same `handle_command()` function, so the server treats them identically.

A third path exists: a **`ChatMessage` (0x09)** whose text starts with `/` is
not broadcast as chat; `handle_chat_message` strips the `/` and routes the rest
through `handle_command()`, so it gets the same reply as the `ChatCommand`. The
vanilla client sends every line typed with `/` as `ChatCommand` or
`SignedChatCommand`, even for a command missing from its tree; this path serves
clients and bots that send commands as chat lines.

### Key types at a glance

| Type              | Source file          | Purpose                                              |
| ----------------- | -------------------- | ---------------------------------------------------- |
| `CommandRegistry` | `commands/mod.rs`    | Stores all registered commands by name and alias     |
| `CommandBuilder`  | `commands/mod.rs`    | Fluent API to define and register commands           |
| `CommandContext`  | `commands/mod.rs`    | Passed to every handler — ECS world access + helpers |
| `ArgParser`       | `commands/parser.rs` | Trait: parse one string token into a typed value     |
| `ParseContext`    | `commands/parser.rs` | Executor entity + world handed to `parse_in`         |
| `CommandQueue`    | `commands/mod.rs`    | ECS resource — FIFO queue of pending commands        |
| `PacketEvent<T>`  | `network.rs`         | Bevy ECS event wrapping a decoded serverbound packet |

## CommandBuilder API

Build commands using the fluent `CommandBuilder`:

```rust
use std::sync::Arc;
use voidmc::{CommandBuilder, GameProfileArg, GreedyStringArg, IntegerArg, StringArg};

let command = CommandBuilder::new("greet")
    .description("Greet a player with a custom message")
    .alias("hello")
    .arg("player", Arc::new(GameProfileArg))
    .arg_optional("count", IntegerArg::new(1, 10))
    .arg_variadic("message", Arc::new(GreedyStringArg))
    .flag("loud", Some('l'), "Send in uppercase")
    .flag_value("color", Some('c'), "Message color", StringArg::single_word())
    .handler(|ctx| {
        let player = ctx.get::<String>("player").unwrap();
        let count = ctx.get::<i32>("count").copied().unwrap_or(1);
        let message = ctx.get::<String>("message")
            .cloned()
            .unwrap_or_else(|| "Hello!".to_string());
        let loud = ctx.flag("loud");
        let color = ctx.flag_value::<String>("color");

        let text = if loud { message.to_uppercase() } else { message };
        for _ in 0..count {
            ctx.reply(&format!("{}: {}", player, text));
        }
    })
    .build();
```

### Builder Methods

| Method                                  | Description                                                                     |
| --------------------------------------- | ------------------------------------------------------------------------------- |
| `new(name)`                             | Create a command with the given name                                            |
| `description(desc)`                     | Set the help description                                                        |
| `alias(alias)`                          | Add an alternative name (can be called multiple times)                          |
| `usage(usage)`                          | Set a custom usage string (overrides auto-generation)                           |
| `arg(name, parser)`                     | Add a required typed argument                                                   |
| `arg_optional(name, parser)`            | Add an optional typed argument                                                  |
| `arg_variadic(name, parser)`            | Add an optional variadic argument (consumes all remaining tokens; must be last) |
| `arg_variadic_required(name, parser)`   | Add a required variadic argument (at least one token; must be last)             |
| `flag(long, short, description)`        | Add a boolean flag (`--long` / `-s`)                                            |
| `flag_value(long, short, desc, parser)` | Add a flag that takes a typed value (`--long value`)                            |
| `subcommand(builder)`                   | Add a literal branch after this command's arguments (see below)                 |
| `requires(fn(&World, Entity) -> bool)`  | Who may run it; checked before parsing and completion (see below)               |
| `handler(fn)`                           | Set the handler function                                                        |
| `build()`                               | Consume the builder and produce a `Command`                                     |

### Subcommands

`subcommand` adds a literal branch, the shape of vanilla commands like
`/team modify <team> color <value>`. A subcommand is a full `CommandBuilder`
(arguments, aliases, flags, its own subcommands) whose literal is tried after
the parent's arguments; every argument along the path lands in the same
`CommandContext`:

```rust
let team = CommandBuilder::new("team")
    .subcommand(
        CommandBuilder::new("add")
            .arg("team", StringArg::single_word())
            .handler(|ctx| { /* ctx.get::<String>("team") */ }),
    )
    .subcommand(
        CommandBuilder::new("modify")
            .arg("team", StringArg::single_word())
            .subcommand(
                CommandBuilder::new("color")
                    .arg("value", Arc::new(ColorArg))
                    .handler(|ctx| { /* "team" and "value" are both set */ }),
            ),
    )
    .build();
```

- A command needs a handler, subcommands, or both. With both, the handler runs
  when no literal follows (`/scoreboard objectives modify <o> numberformat`
  clears, `... numberformat blank` takes the branch).
- Arguments before subcommands must be required and not variadic, so the
  literal position is never ambiguous.
- Flags declared on a parent apply to its subcommands; put them after the
  subcommand literal.
- `description` and `suggest_entity_types` are read on the top-level command
  only.
- Errors name the missing or unknown subcommand, and the usage line follows
  the resolved path (`/team modify <team:team> <displayName|color|...>`).
- The client tree carries real literal nodes, so literals complete locally
  and arguments after them use their own parser and suggestions.

### Requirements

`requires` is brigadier's permission check. It runs before any argument is
parsed or completed, on the command and on every subcommand along the path, so
a refused player gets one "You do not have permission to use this command"
reply and no suggestions, never an argument error that would reveal state:

```rust
CommandBuilder::new("stop")
    .requires(|world, player| world.get::<Operator>(player).is_some())
    .handler(|ctx| { /* ... */ })
    .build();
```

A player is sent only the commands and subcommands whose requirements accept
it, in its command tree and in `/help`, like on a vanilla server. The tree is
built when the player joins and rebuilt when `Operator` is added to or removed
from it (see [Permissions](#permissions)); a requirement that reads anything
else applies at once to dispatch and completion, but the client's tree only
follows at the next join or `Operator` change.

## CommandContext

The `CommandContext` is passed to every command handler, providing access to the ECS world and helper methods:

```rust
pub struct CommandContext<'a> {
    pub world: &'a mut World,    // Full ECS world access
    pub entity: Entity,          // The player entity who ran the command
    pub client_id: u32,          // Network client ID
    pub args: Vec<String>,       // Raw argument tokens
}
```

### Methods

| Method                  | Return Type      | Description                                      |
| ----------------------- | ---------------- | ------------------------------------------------ |
| `get::<T>(name)`        | `Option<&T>`     | Get a parsed argument by name and type           |
| `has_arg(name)`         | `bool`           | Check if an optional argument was provided       |
| `flag(name)`            | `bool`           | Check if a boolean flag is set                   |
| `flag_value::<T>(name)` | `Option<&T>`     | Get a typed flag value                           |
| `reply(message)`        | `()`             | Send a white system message to the sender        |
| `reply_error(message)`  | `()`             | Send a red error message to the sender           |
| `broadcast(message)`    | `()`             | Send a system message to all ready players       |
| `player_name()`         | `Option<String>` | Get the sender's player name                     |
| `is_operator()`         | `bool`           | Check if the sender has the `Operator` component |

## Argument Types

Every argument is an `Arc<dyn ArgParser>`. Each parser declares the vanilla
Brigadier parser the client should use (so the client validates and colours
the token locally), how many whitespace tokens it consumes, and the typed value
the handler reads back with `ctx.get::<T>(name)`.

| Parser | `ctx.get::<T>` | Client parser | Accepts |
|---|---|---|---|
| `StringArg::single_word()` | `String` | `brigadier:string` (single word) | One word |
| `StringArg::quotable()` | `String` | `brigadier:string` (quotable) | One word or a quoted phrase |
| `StringArg::greedy()` / `GreedyStringArg` | `String` | `brigadier:string` (greedy) | The rest of the line |
| `BoolArg` | `bool` | `brigadier:bool` | `true/false/yes/no/1/0` |
| `IntegerArg::new(min, max)` / `::min(min)` / `::unbounded()` | `i32` | `brigadier:integer` + bounds | Bounded integer |
| `LongArg::new(min, max)` / `::min(min)` / `::unbounded()` | `i64` | `brigadier:long` + bounds | Bounded long |
| `FloatArg::new(min, max)` / `::min(min)` / `::unbounded()` | `f32` | `brigadier:float` + bounds | Finite float with optional bounds |
| `DoubleArg::new(min, max)` / `::min(min)` / `::unbounded()` | `f64` | `brigadier:double` + bounds | Finite double with optional bounds |
| `Vec3Arg` | `[f64; 3]` | `minecraft:vec3` | `x y z` with `~` and `^` forms (3 tokens) |
| `BlockPosArg` | `[i32; 3]` | `minecraft:block_pos` | Integer `x y z` with `~` and `^` forms (3 tokens) |
| `PlayerArg` | `Entity` | `minecraft:entity` (single, players) | `@s`, `@p`, `@r` or a player name |
| `PlayersArg` | `Vec<Entity>` | `minecraft:entity` (players) | `@a`, `@s`, `@p`, `@r` or a player name |
| `GameProfileArg` | `String` | `minecraft:game_profile` | A player name, completed from the server |
| `EntityArg` | `String` | `minecraft:entity` | Raw selector text, not resolved |
| `ItemArg` | `ItemId` | `minecraft:item_stack` | `minecraft:stone` or `stone` |
| `BlockArg` | `i32` (default block-state id) | `minecraft:block_state` | `minecraft:stone` or `stone` |
| `ColorArg` | `String` | `minecraft:color` | One of the 16 vanilla colour names or `reset` |
| `GameModeArg` | `GameMode` | `minecraft:gamemode` | `survival`, `creative`, `adventure`, `spectator` or `0`-`3` |
| `DimensionArg` | `DimensionId` | `minecraft:dimension` | `minecraft:overworld`, `the_nether`, `the_end` |
| `TimeArg::new(min)` / `::non_negative()` | `i32` ticks | `minecraft:time` + min | `20`, `20t`, `1.5s`, `2d` |
| `UuidArg` | `uuid::Uuid` | `minecraft:uuid` | Hyphenated or plain hex UUID |
| `EnumArg::new([(name, value), ..])` | `T` | `brigadier:string` + `ask_server` | One of the declared names |
| `ResourceLocationArg` | `String` | `minecraft:resource_location` | `namespace:path` |
| `SummonableEntityArg` | `String` | `minecraft:resource_location` + `summonable_entities` | Entity type id |
| `MessageArg` | `String` | `minecraft:message` | Chat message |

The `voidmc-vanilla-commands` crate adds parsers for the scoreboard family
(`TeamArg`, `ObjectiveArg`, `ScoreHolderArg`, `OperationArg`, `SlotArg`,
`CriteriaArg`, `ComponentArg`, `StyleArg`); see
[Vanilla /team & /scoreboard](vanilla-commands.md#argument-types). Their client
parsers (`minecraft:team`, `objective`, `score_holder`, `operation`,
`scoreboard_slot`, `objective_criteria`) are `Parser` variants like the rest.

Parser ids and property encodings follow the 26.1.2
`minecraft:command_argument_type` registry; a wrong id disconnects the client
on join, so `voidmc_protocol::clientbound::commands::Parser` is byte-tested
against Paper.

### Coordinates

`Vec3Arg` and `BlockPosArg` resolve against the executor when the handler runs:

- `10 64 -3` — absolute (`BlockPosArg` requires integers here);
- `~ ~1 ~-2` — relative to the executor's `Position`;
- `^ ^ ^2` — local to the executor's look direction (`left up forward`);
  all three coordinates must be local, mixing is rejected.

`BlockPosArg` floors the resolved position. Absolute integers are **not**
centred (`/tp 10 64 10` goes to `10.0, 64.0, 10.0`, where vanilla centres x
and z to `10.5, 64.0, 10.5`).

```rust
CommandBuilder::new("setblock")
    .arg("pos", Arc::new(BlockPosArg))
    .arg("block", Arc::new(BlockArg))
    .handler(|ctx| {
        let [x, y, z] = *ctx.get::<[i32; 3]>("pos").unwrap();
        let state = *ctx.get::<i32>("block").unwrap();
        ctx.reply(&format!("{state} at {x} {y} {z}"));
    })
```

### Player selectors

`PlayerArg` / `PlayersArg` parse the selector and resolve it to ready player
entities before the handler runs, so a missing player is a parse error with
the usual red message and usage line:

| Selector | `PlayerArg` | `PlayersArg` |
|---|---|---|
| `Name` (case-insensitive) | that player | `[that player]` |
| `@s` | the executor | `[executor]` |
| `@p` | nearest ready player in the executor's dimension (including itself) | `[nearest]` |
| `@r` | a random ready player | `[random]` |
| `@a` | rejected (several players) | every ready player |

Selector arguments (`@a[distance=..3]`) and `@e` are not supported.

```rust
CommandBuilder::new("heal")
    .arg("targets", Arc::new(PlayersArg))
    .handler(|ctx| {
        let targets = ctx.get::<Vec<Entity>>("targets").unwrap().clone();
        ctx.reply(&format!("Healed {} player(s)", targets.len()));
    })
```

### Enumerations

`EnumArg` maps a fixed set of names to values of any `Clone + Send + Sync`
type. The usage line shows the variants (`<team:red|blue>`) and the client asks
the server for completions:

```rust
#[derive(Clone, Copy)]
enum Team { Red, Blue }

CommandBuilder::new("team")
    .arg("team", EnumArg::new([("red", Team::Red), ("blue", Team::Blue)]))
    .handler(|ctx| {
        let team = *ctx.get::<Team>("team").unwrap();
        // ...
    })
```

## Custom ArgParser

Implement the `ArgParser` trait to create custom argument types. Override
`parse_in` instead of `parse` when the value depends on the executor, and
return `minecraft:ask_server` from `suggestions_type` to have the client ask
the server for completions (`suggestions` then runs with the partial token):

```rust
use std::any::Any;
use voidmc::{ArgParser, ParseContext};
use voidmc_protocol::clientbound::commands::{Parser, StringType};

pub struct WarpArg;

impl ArgParser for WarpArg {
    fn type_name(&self) -> &str { "warp" }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        match input {
            "spawn" | "arena" => Ok(Box::new(input.to_string())),
            _ => Err(format!("'{}' is not a known warp", input)),
        }
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::String(StringType::SingleWord))
    }

    fn suggestions(&self, partial: &str, _world: &bevy_ecs::world::World) -> Vec<String> {
        ["spawn", "arena"]
            .iter()
            .filter(|c| c.starts_with(partial))
            .map(|c| c.to_string())
            .collect()
    }

    fn suggestions_type(&self) -> Option<&str> {
        Some("minecraft:ask_server")
    }
}
```

## Flag System

Flags are parsed in a pre-pass before positional arguments:

- `--flag` — Boolean flag (sets to `true`)
- `--flag value` — Single-token value flag (parsed with the flag's
  `ArgParser::parse_in`, so `PlayerArg` selectors like `@s` work as flag values)
- `-f` — Short boolean flag
- `-f value` — Short value flag (must be standalone, not combined)
- `--` — Stop flag parsing; everything after is positional

A command (or subcommand path) that declares no flags skips the pre-pass, so
`-blue` or `--` are ordinary arguments. When the last argument is variadic,
flags are only read before it starts: once the earlier arguments are filled,
the rest of the line, dashes included, belongs to it. A flag right before the
first word of that argument still counts (`/sign -g hello`).

Example:

```
/mycommand --verbose -c red -- some positional args
```

## Default Commands

Register all built-in commands with `register_default_commands`:

```rust
use voidmc::{CommandRegistry, register_default_commands};

let mut registry = app.world_mut().resource_mut::<CommandRegistry>();
register_default_commands(&mut registry, &[]);
```

Pass command names in the `exclude` slice to skip specific defaults:

```rust
register_default_commands(&mut registry, &["kick", "gamemode"]);
```

### Permissions

The defaults that act on the world, the server or other players require the
`Operator` component on the sender: `/broadcast`, `/clear`, `/gamemode`
(`/gm`), `/give`, `/kick`, `/say`, `/stop`, `/summon` and `/tp`. `/help`,
`/list`, `/ping`, `/plugins` (`/pl`) and `/tell` (`/msg`) are open to every
player. A player without `Operator` who runs an operator command gets
`You do not have permission to use this command` in red, and nothing else
happens.

Nobody is an operator by default. An application grants it by inserting the
component on the player's entity and revokes it by removing it, at any time:

```rust
use voidmc::components::{Operator, PlayerName};
use voidmc::events::PlayerJoinEvent;

fn op_admins(event: On<PlayerJoinEvent>, names: Query<&PlayerName>, mut commands: Commands) {
    if names.get(event.entity).is_ok_and(|name| name.0 == "Alice") {
        commands.entity(event.entity).insert(Operator);
    }
}
```

`Operator` is a marker without a level. Like a vanilla server, VoidMC tells
each client its permission level with the entity event `24 + level` on the
client's own player: 4 (`28`) for an operator, 0 (`24`) for anyone else; the
vanilla client uses it to decide, among other things, whether to offer entity
selectors such as `@a`. The level and then the player's command tree are sent
when it joins and again whenever `Operator` is added to or removed from it, so
its tab completion and `/help` follow at once. Inserting `Operator` on a
player that already has it sends nothing.

To let other players run a default command, or to restrict it differently,
exclude it and register your own command under the same name, with your own
`requires` or none:

```rust
register_default_commands(&mut registry, &["tp"]);
registry.register(
    CommandBuilder::new("tp")
        .description("Teleport to coordinates")
        .arg("position", Arc::new(Vec3Arg))
        .handler(|ctx| { /* ... */ })
        .build(),
);
```

### Available Default Commands

The Description column is what `/help` shows.

| Command | Aliases | Description | Arguments |
|---|---|---|---|
| `/help` | | List available commands | `[command:string]` |
| `/gamemode` | `/gm` | Change game mode | `<mode:gamemode>` |
| `/kick` | | Kick a player | `<player:player> [reason:text]...` (selectors: `@s`, `@p`, `@r`, name) |
| `/ping` | | Pong! | (none) |
| `/plugins` | `/pl` | List plugins | (none) |
| `/tp` | | Teleport to coordinates | `<position:vec3>` (`~` and `^` forms); keeps the look direction; rejects positions outside ±30,000,000 horizontally or ±20,000,000 vertically |
| `/broadcast` | | Broadcast a message to all players | `<message:text>...` |
| `/tell` | `/msg` | Send a private message to a player | `<player:player> <message:text>...` (selectors: `@s`, `@p`, `@r`, name) |
| `/list` | | List online players | (none) |
| `/say` | | Send a message as yourself | `<message:text>...` |
| `/summon` | | Summon an entity at a position | `<entity:resource_location> [position:vec3] [--wander] [--gravity] [--block-checks]`; rejects positions outside the `/tp` bounds (±30,000,000 horizontally, ±20,000,000 vertically) with "Invalid position for summon" |
| `/give` | | Give yourself an item | `<item:item> [count:integer(1..64)]` |
| `/clear` | | Clear your inventory | (none) |
| `/stop` | | Gracefully stop the server | (none) |

`/stop` emits an `AppExit`, so the server finishes the current tick and shuts
down cleanly. Plugins that clean up on shutdown — such as
[world serialization](/reference/server/world-serialization), which flushes
dirty chunks — react to the same signal. Ctrl-C (SIGINT), SIGTERM and SIGHUP
stop the server the same way. `VoidServer::run` then returns `AppExit::Success`
after `/stop` and `AppExit::from_code(130)` after a signal (see
[Stopping and the exit code](/reference/server/configuration#stopping-and-the-exit-code)).

`/kick` sends the reason in a `Disconnect` packet, then the server closes the
connection itself, so a client that ignores the packet does not stay in the
game (see [Kicking a player](/reference/server/sending-packets#kicking-a-player)).

The player-management commands answer as follows. Every player counts as
online once it is ready (it has loaded the world), the same set the tab list
and the server-list player count show; a player still loading is not listed
and cannot be named as a target. `/list` and `/tell` are open to every
player; `/broadcast`, `/say` and `/kick` require `Operator` (see
[Permissions](#permissions)).

| Command | Sender sees | Others see |
|---|---|---|
| `/list` | `There are 2 player(s) online: Alice, Bob` (`There are 0 player(s) online.` when empty) | nothing |
| `/tell Bob hi` | `You whisper to Bob: hi` | Bob only: `Alice whispers to you: hi` (gray) |
| `/broadcast hi` | `[Broadcast] hi` | every online player: `[Broadcast] hi` |
| `/say hi` | `[Alice] hi` | every online player: `[Alice] hi` |
| `/kick Bob [reason]` | `Kicked Bob (reason: ...)` | Bob is disconnected with the reason in red (default `Kicked by an operator`); the others see Bob leave the world and the tab list |

Errors (unknown or missing player, missing text) go to the sender only, in
red, followed by the usage line. `/tell`, `/broadcast` and `/say` refuse a
message, and `/kick` a reason, containing a `§` formatting code or a control
character (`Illegal characters in chat`), as a vanilla server does, so the
line shows exactly as typed. A refused message reaches nobody and a refused
`/kick` kicks nobody.
Message words are joined by single spaces: runs of spaces and line breaks in
the command become one space.

`/summon` accepts only full namespaced entity IDs and validates them against
the server's versioned `minecraft:entity_type` data. Coordinates are grouped:
either omit all three to use the executor position, or provide `x`, `y`, and
`z` together. Partial coordinate input is rejected instead of being silently
ignored.

The optional `--wander` flag attaches the demo random-walk behavior to the
summoned entity. `--gravity` enables the simple server-side vertical physics,
and `--block-checks` enables world block collision checks for that physics
step. These flags are stored as ECS movement components and synchronized by the
non-player entity lifecycle. With `--gravity` alone the entity falls through
blocks, and it is removed once it is more than 64 blocks below the bottom of
the world (see [Entities](/reference/gameplay/entities#gravity-and-block-collision)).

`/give` tops up matching stacks first, then fills empty slots (hotbar before
the main inventory), never past the item's maximum stack size: 20 ender pearls
become 16 + 4. Items that do not fit are not given, and the reply says how many
did not fit. `minecraft:air` is refused. `/clear` empties every slot of the
player inventory (crafting grid, armor, main inventory, hotbar, offhand) and the
carried item, then resends the whole window.

### `/gamemode`

`/gamemode <mode>` changes only the game mode the sender's client displays,
nothing the server enforces. `mode` is `survival`, `creative`, `adventure` or
`spectator` (any case) or the legacy id `0`-`3`. On success the server:

- sends the sender a Game Event `change_game_mode` with the mode id, so the
  client switches its local mode. The vanilla client derives flight and
  instant break from that mode itself; no Player Abilities packet is sent;
- sets the sender's `TabEntry::game_mode`, so every player, the sender
  included, receives a Player Info Update with `UPDATE_GAME_MODE` (a
  spectator shows greyed out in the tab list);
- replies `Game mode set to <name> (<id>)`, for example
  `Game mode set to creative (1)`.

There is no teleport and no respawn. Any other value (`4`, `-1`, `99`,
`hardcore`, a missing mode or an extra argument) is a parse error: the
sender gets the error in red, such as `Invalid value '4' for <mode>: expected
gamemode ('4' is not a valid game mode (expected survival, creative, adventure
or spectator))`, `Missing required argument <mode> (expected gamemode)` or
`Too many arguments: expected 1, got 2`, followed by
`Usage: /gamemode <mode:gamemode>` in gray, and nothing changes.

The argument is advertised as `minecraft:gamemode`, so the vanilla client
shows anything but the four names in red as you type, the numeric ids `0`-`3`
included, since vanilla only knows the names. Pressing Enter still sends the
command, and the server accepts those ids.

The server keeps applying the rules of the configured
[`game_mode`](../server/configuration.md) to every player: block
breaking, the reach of a click on a block, the item a placed block uses up,
and the creative-only inventory clicks (cloning a stack, middle-button drags)
follow it, not `/gamemode`. On a Survival
server, a player shown Creative removes a block locally when it starts digging,
but the server treats that as the start of a survival dig, so the block comes
back. The mode is not stored either: a player who reconnects starts in the
configured mode again.

### PluginList Resource

`/plugins` (alias `/pl`) lists the names in the optional `PluginList`
resource, in the order given. VoidMC neither fills it nor discovers plugins:
the plugins an application adds with `add_plugin` are not listed unless it
names them here. Insert it to show players the names you choose:

```rust
use voidmc::PluginList;

VoidServer::new(config)
    .add_plugin(|app| {
        app.insert_resource(PluginList(vec![
            "MyPlugin".to_string(),
            "AnotherPlugin".to_string(),
        ]));
    })
```

| `PluginList` | `/plugins` reply |
|---|---|
| not inserted, or empty | `No plugins loaded.` |
| `["MyPlugin", "AnotherPlugin"]` | `Plugins (2): MyPlugin, AnotherPlugin` |

### Replies and Invalid Input

Every reply is a system chat message sent to the player who ran the command,
and to nobody else.

- `/help` answers with one message: a `--- Available Commands ---` header, then
  one line per command, `  /name - description` (`  /name` without a
  description), sorted by name. It reads the `CommandRegistry` when it runs, so
  a command registered after the player joined is listed too. Each command
  appears once under its name; aliases are not listed, but they are in the
  client's command tree and `/help <alias>` shows their command. Commands whose
  `requires` refuses the player are left out, such as the
  [operator commands](#permissions) for a player without `Operator`. There
  are no pages.
- `/help <command>` (a name or an alias) shows `--- /name ---`, the
  description and `  Usage: <usage>`. Any other word, a page number or a
  command the player may not run included, gets `Unknown command: /<word>` in
  red, the word echoed like an unknown command name (see below).
- `/ping` answers `Pong!`.

Invalid input gets a reply and never disconnects the player:

| Input | Reply |
|---|---|
| A name that is neither a command nor an alias; names are case-sensitive, so `/HELP` is one | `Unknown command: /<name>` in red |
| `/` alone, or only whitespace | `Unknown command: /` in red |
| A missing, invalid or extra argument | each error in red, then `Usage: <usage>` in gray |
| A command whose `requires` refuses the player | `You do not have permission to use this command` in red |

Whitespace around and between words is ignored: `/ help  ping ` runs
`/help ping`. An unknown name is echoed at any length the packet allows,
without its `§` formatting codes (each `§` and the character after it, which
a vanilla client reads as one code) and control characters: `/§cnope` gets
`Unknown command: /nope`, in plain red.

### Example-only Commands

`void-example` registers an additional `/circle` command outside of
`register_default_commands` to demonstrate that gameplay commands can live in an
application/plugin crate instead of in `void` itself.

| Command | Description | Arguments |
|---|---|---|
| `/circle` | Spawn or remove an orbiting entity ring | `[entity:resource_location] [player:player] [--stop]` |
| `/spawn` | Spawn an entity with metadata sugar (`CustomName`, `Glowing`, `Invisible`, `NoGravity`) | `[entity:resource_location] [--name <text>] [--glow] [--invisible] [--float]` |
| `/display` | Display-entity and passenger demos (see [Entities](/reference/gameplay/entities)) | `<shield|sign|item|ride|clear> [text]` |

`/circle` defaults to `minecraft:pig` and the executor and spawns the ring
through `EntityBuilder`. `/circle --stop` despawns the entities directly; the
`RemoveEntities` packets follow from the lifecycle observer like for any other
spawned entity. The command uses the public command API plus public ECS
components from `void`, while its own marker components and movement system
stay local to `void-example`.

## Tab-Completion

The server automatically builds a Minecraft protocol command tree from the `CommandRegistry` and sends it to each client as it enters the play state, right after the `Login` packet and the client's permission level. Each player's tree holds only the commands it may run (see [Requirements](#requirements)). This provides:

- Command name completion (typing `/` shows all commands)
- Client-side validation and completion for every vanilla parser in the table
  above (numbers with bounds, coordinates, selectors, items, blocks, colours,
  game modes, dimensions, times, UUIDs)
- Long and short flag suggestions after the command's required arguments
- Server-side completion through `minecraft:ask_server`: the client sends
  `CommandSuggestionsRequest` with the whole line, `CommandRegistry::complete(text, world, executor)`
  finds the argument under the cursor (skipping flags and their values) and
  calls its parser's `suggestions`, and the reply is a
  `CommandSuggestionsResponse` covering just the partial token. `GameProfileArg`
  (ready player names) and `EnumArg` (variant names) use it out of the box.
- Summon entity suggestions for `SummonableEntityArg` arguments (via
  `minecraft:summonable_entities`), completed by the client from its own list
- Player names for `PlayerArg` and `PlayersArg` (`minecraft:entity`), completed
  by the client from its player list
- Alias support (aliases appear as separate entries pointing to the same argument chain)
- Subcommand literals, completed locally by the client; `complete` walks the
  literals typed so far before picking the argument under the cursor

The command tree is rebuilt from the registry each time a client joins, and
for one player whenever `Operator` is added to or removed from it.

No default command has a `minecraft:ask_server` argument, so a vanilla client
completes them without asking the server. When a client does send
`CommandSuggestionsRequest`, the server answers it as follows:

- The line may start with `/` or not. It is answered only when it names a
  registered command or alias followed by a space; an empty line, a bare name
  (`/he`) or an unknown command gets no reply.
- Otherwise the reply echoes the transaction id and always comes, with an
  empty list when the argument under the cursor does not ask the server, or
  when the line is past the last argument.
- The built-in parsers (`EnumArg`, `GameProfileArg`, subcommand literals)
  match a prefix ignoring ASCII case; flags match it exactly. Matches keep the
  parser's order and are not sorted.
- Every player in the play state is answered, ready or not; `GameProfileArg`
  offers the ready players only, the requester included.

## Registration Methods

Commands can be registered in two ways:

### Via `add_command` on VoidServer

```rust
VoidServer::new(config)
    .add_command(CommandBuilder::new("hello").handler(|ctx| ctx.reply("Hi!")).build())
```

### Via `add_plugin` with direct registry access

```rust
VoidServer::new(config)
    .add_plugin(|app| {
        let mut registry = app.world_mut().resource_mut::<CommandRegistry>();
        registry.register(my_command());
        register_default_commands(&mut registry, &[]);
    })
```
