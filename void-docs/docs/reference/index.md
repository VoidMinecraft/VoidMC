# Reference

Void is a Minecraft-compatible server framework written in Rust. It combines [Bevy ECS](https://bevyengine.org/) for game logic with [Tokio](https://tokio.rs/) for asynchronous networking, giving you a modular, high-performance foundation for building custom Minecraft servers.

## Crate Structure

| Crate | Description |
|---|---|
| **`void`** | Core server framework — ECS components, systems, handlers, commands, world generation, and the public API. |
| **`void-net`** | Low-level TCP socket abstraction (accept, read, write framed packets). |
| **`void-protocol`** | Minecraft protocol definitions — serverbound/clientbound packet enums and data types. |
| **`void-codec`** | Binary `Encode`/`Decode` traits and primitive type implementations. |
| **`void-codec-macros`** | Derive macros (`#[derive(Encode, Decode)]`) and field attributes for codec automation. |

## Quick Start

Add `void` as a dependency, then create a minimal server:

```rust
use void::{
    CommandBuilder, CommandRegistry, ServerBuilder, VoidServer,
    register_default_commands,
};

fn main() {
    VoidServer::new(ServerBuilder::new().build())
        .add_plugin(|app| {
            let mut registry = app.world_mut().resource_mut::<CommandRegistry>();
            register_default_commands(&mut registry, &[]);
        })
        .add_command(
            CommandBuilder::new("hello")
                .description("Greet the player")
                .handler(|ctx| {
                    ctx.reply("Hello from my server!");
                })
                .build(),
        )
        .run();
}
```

This starts a server on `127.0.0.1:25565` with default settings, all built-in commands, and a custom `/hello` command.

## Server

- [Architecture](/reference/server/architecture) — Dual-threaded model, tick loop, plugin system, packet flow
- [Configuration](/reference/server/configuration) — `ServerBuilder` API, `ServerConfig` fields, defaults
- [ECS Components & Resources](/reference/server/ecs) — All components, resources, and entity lifecycle
- [Events](/reference/server/events) — Semantic events, packet events, observer pattern

## Protocol & Codec

- [Networking & Protocol](/reference/protocol/networking) — Protocol states, connection lifecycle, keep-alive
- [Binary Codec](/reference/protocol/codec) — `Encode`/`Decode` traits, derive macros, field attributes

## Gameplay

- [Command System](/reference/gameplay/commands) — `CommandBuilder`, argument parsers, flags, default commands
- [Player Management](/reference/gameplay/players) — Join/quit flow, visibility, position broadcasting, teleportation
- [World & Chunks](/reference/gameplay/world) — Chunk system, dimensions, streaming, world generators
- [Registry System](/reference/gameplay/registry) — `RegistryDataStore` API, default registries, customization
