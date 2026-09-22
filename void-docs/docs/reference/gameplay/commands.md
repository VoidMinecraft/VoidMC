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
  ├── flags::extract_flags() — peels off --flags / -f tokens
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

A third path exists: if the client types a command that is **not in its local command tree** (i.e. a command the server registered after the client received the tree), it sends a **`ChatMessage` (0x09)** prefixed with `/`. `handle_chat_message` intercepts that case and routes it through `handle_command()` as well.

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
use voidmc::{CommandBuilder, GameProfileArg, GreedyStringArg, IntegerArg};

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
| `handler(fn)`                           | Set the handler function                                                        |
| `build()`                               | Consume the builder and produce a `Command`                                     |

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
| `FloatArg::new(min, max)` / `::min(min)` / `::unbounded()` | `f32` | `brigadier:float` + bounds | Bounded float |
| `DoubleArg::new(min, max)` / `::min(min)` / `::unbounded()` | `f64` | `brigadier:double` + bounds | Bounded double |
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
centred (`/tp 10 64 10` goes to `10.0, 64.0, 10.0`, unlike vanilla's `10.5`).

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
| `@p` | nearest ready player to the executor (including itself) | `[nearest]` |
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
- `--flag value` — Value flag (parsed with the flag's `ArgParser`)
- `-f` — Short boolean flag
- `-f value` — Short value flag (must be standalone, not combined)
- `--` — Stop flag parsing; everything after is positional

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

### Available Default Commands

| Command | Aliases | Description | Arguments |
|---|---|---|---|
| `/help` | | List commands or show command details | `[command:string]` |
| `/gamemode` | `/gm` | Change game mode | `<mode:gamemode>` |
| `/kick` | | Kick a player | `<player:player> [reason:text]...` (selectors: `@s`, `@p`, `@r`, name) |
| `/ping` | | Pong! | (none) |
| `/plugins` | `/pl` | List loaded plugins | (none) |
| `/tp` | | Teleport to coordinates | `<position:vec3>` (`~` and `^` forms) |
| `/broadcast` | | Broadcast to all players | `<message:text>...` |
| `/tell` | `/msg` | Private message a player | `<player:player> <message:text>...` (selectors: `@s`, `@p`, `@r`, name) |
| `/list` | | Show online players | (none) |
| `/say` | | Send a message as yourself | `<message:text>...` |
| `/summon` | | Spawn a non-player entity | `<entity:resource_location> [position:vec3] [--wander] [--gravity] [--block-checks]` |
| `/give` | | Give yourself an item | `<item:item> [count:integer(1..64)]` |
| `/clear` | | Empty your inventory | (none) |
| `/stop` | | Gracefully stop the server | (none) |

`/stop` emits an `AppExit`, so the server finishes the current tick and shuts
down cleanly. Plugins that clean up on shutdown — such as
[world serialization](/reference/server/world-serialization), which flushes
dirty chunks — react to the same signal. Ctrl-C / SIGTERM stop the server the
same way.

`/summon` accepts only full namespaced entity IDs and validates them against
the server's versioned `minecraft:entity_type` data. Coordinates are grouped:
either omit all three to use the executor position, or provide `x`, `y`, and
`z` together. Partial coordinate input is rejected instead of being silently
ignored.

The optional `--wander` flag attaches the demo random-walk behavior to the
summoned entity. `--gravity` enables the simple server-side vertical physics,
and `--block-checks` enables world block collision checks for that physics
step. These flags are stored as ECS movement components and synchronized by the
non-player entity lifecycle.

### PluginList Resource

The `/plugins` command reads from an optional `PluginList` resource. Insert it in your plugin to make plugin names visible:

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

The server automatically builds a Minecraft protocol command tree from the `CommandRegistry` and sends it to clients during the configuration phase. This provides:

- Command name completion (typing `/` shows all commands)
- Client-side validation and completion for every vanilla parser in the table
  above (numbers with bounds, coordinates, selectors, items, blocks, colours,
  game modes, dimensions, times, UUIDs)
- Long and short flag suggestions after the command's required arguments
- Server-side completion through `minecraft:ask_server`: the client sends
  `CommandSuggestionsRequest` with the whole line, `CommandRegistry::complete`
  finds the argument under the cursor (skipping flags and their values) and
  calls its parser's `suggestions`, and the reply is a
  `CommandSuggestionsResponse` covering just the partial token. `GameProfileArg`
  (ready player names) and `EnumArg` (variant names) use it out of the box.
- Summon entity suggestions for `SummonableEntityArg` arguments (via `minecraft:summonable_entities`)
- Alias support (aliases appear as separate entries pointing to the same argument chain)

The command tree is rebuilt from the registry each time a client joins.

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
