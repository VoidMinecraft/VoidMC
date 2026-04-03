# Command System

## Overview

Void includes a full command system with typed argument parsing, flag support, auto-generated usage strings, and client-side tab-completion via the Minecraft protocol command tree.

## CommandBuilder API

Build commands using the fluent `CommandBuilder`:

```rust
use std::sync::Arc;
use void::{CommandBuilder, GameProfileArg, GreedyStringArg, IntegerArg};

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

| Method | Description |
|---|---|
| `new(name)` | Create a command with the given name |
| `description(desc)` | Set the help description |
| `alias(alias)` | Add an alternative name (can be called multiple times) |
| `usage(usage)` | Set a custom usage string (overrides auto-generation) |
| `arg(name, parser)` | Add a required typed argument |
| `arg_optional(name, parser)` | Add an optional typed argument |
| `arg_variadic(name, parser)` | Add an optional variadic argument (consumes all remaining tokens; must be last) |
| `arg_variadic_required(name, parser)` | Add a required variadic argument (at least one token; must be last) |
| `flag(long, short, description)` | Add a boolean flag (`--long` / `-s`) |
| `flag_value(long, short, desc, parser)` | Add a flag that takes a typed value (`--long value`) |
| `handler(fn)` | Set the handler function |
| `build()` | Consume the builder and produce a `Command` |

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

| Method | Return Type | Description |
|---|---|---|
| `get::<T>(name)` | `Option<&T>` | Get a parsed argument by name and type |
| `has_arg(name)` | `bool` | Check if an optional argument was provided |
| `flag(name)` | `bool` | Check if a boolean flag is set |
| `flag_value::<T>(name)` | `Option<&T>` | Get a typed flag value |
| `reply(message)` | `()` | Send a white system message to the sender |
| `reply_error(message)` | `()` | Send a red error message to the sender |
| `broadcast(message)` | `()` | Send a system message to all ready players |
| `player_name()` | `Option<String>` | Get the sender's player name |
| `is_operator()` | `bool` | Check if the sender has the `Operator` component |

## Argument Parsers

Built-in parsers that implement the `ArgParser` trait:

| Parser | Parsed Type | Protocol Hint | Description |
|---|---|---|---|
| `StringArg::single_word()` | `String` | `SingleWord` | Single whitespace-delimited word |
| `StringArg::quotable()` | `String` | `QuotablePhrase` | Quoted or single word |
| `StringArg::greedy()` | `String` | `GreedyPhrase` | All remaining input |
| `IntegerArg::new(min, max)` | `i32` | `Integer { min, max }` | Bounded integer |
| `IntegerArg::unbounded()` | `i32` | `Integer` | Unbounded integer |
| `LongArg::new(min, max)` | `i64` | `Long { min, max }` | Bounded long integer |
| `LongArg::unbounded()` | `i64` | `Long` | Unbounded long |
| `FloatArg::new(min, max)` | `f32` | `Float { min, max }` | Bounded float |
| `FloatArg::unbounded()` | `f32` | `Float` | Unbounded float |
| `DoubleArg::new(min, max)` | `f64` | `Double { min, max }` | Bounded double |
| `DoubleArg::unbounded()` | `f64` | `Double` | Unbounded double |
| `BoolArg` | `bool` | `Bool` | Accepts `true/false/yes/no/1/0` |
| `GreedyStringArg` | `String` | `GreedyPhrase` | All remaining input as text |
| `GameProfileArg` | `String` | `GameProfile` | Player name with tab-completion (`minecraft:ask_server`) |
| `EntityArg::single_player()` | `String` | `Entity { single, players_only }` | Entity selector |
| `MessageArg` | `String` | `Message` | Chat message argument |

## Custom ArgParser

Implement the `ArgParser` trait to create custom argument types:

```rust
use std::any::Any;
use void::ArgParser;
use void_protocol::clientbound::commands::Parser;

pub struct ColorArg;

impl ArgParser for ColorArg {
    fn type_name(&self) -> &str { "color" }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        match input {
            "red" | "green" | "blue" | "white" => Ok(Box::new(input.to_string())),
            _ => Err(format!("'{}' is not a valid color", input)),
        }
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::String(void_protocol::clientbound::commands::StringType::SingleWord))
    }

    // Optional: provide tab-completion suggestions
    fn suggestions(&self, partial: &str, _world: &bevy_ecs::world::World) -> Vec<String> {
        ["red", "green", "blue", "white"]
            .iter()
            .filter(|c| c.starts_with(partial))
            .map(|c| c.to_string())
            .collect()
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
use void::{CommandRegistry, register_default_commands};

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
| `/gamemode` | `/gm` | Change game mode | `<mode:integer(0..3)>` |
| `/kick` | | Kick a player | `<player:player> [reason:text]...` |
| `/ping` | | Pong! | (none) |
| `/plugins` | `/pl` | List loaded plugins | (none) |
| `/tp` | | Teleport to coordinates | `<x:double> <y:double> <z:double>` |
| `/broadcast` | | Broadcast to all players | `<message:text>...` |
| `/tell` | `/msg` | Private message a player | `<player:player> <message:text>...` |
| `/list` | | Show online players | (none) |
| `/say` | | Send a message as yourself | `<message:text>...` |

### PluginList Resource

The `/plugins` command reads from an optional `PluginList` resource. Insert it in your plugin to make plugin names visible:

```rust
use void::PluginList;

VoidServer::new(config)
    .add_plugin(|app| {
        app.insert_resource(PluginList(vec![
            "MyPlugin".to_string(),
            "AnotherPlugin".to_string(),
        ]));
    })
```

## Tab-Completion

The server automatically builds a Minecraft protocol command tree from the `CommandRegistry` and sends it to clients during the configuration phase. This provides:

- Command name completion (typing `/` shows all commands)
- Argument type hints (integers, strings, players, etc.)
- Player name suggestions for `GameProfileArg` arguments (via `minecraft:ask_server`)
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
