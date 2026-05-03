# Commands

The server publishes its complete Brigadier command graph to the client with the `Commands` packet (clientbound, Play state). The client uses this graph to drive in-chat tab-completion, parse-time error highlighting, and to filter `/`-prefixed input from the chat box. The client never *executes* commands locally — it sends them to the server as `Chat Command` packets — but it must be able to *recognize* the syntax.

This page covers `Commands`, `Command Suggestions Request`, and `Command Suggestions Response`.

## The Commands packet

The graph is sent as a flat array of nodes plus a root index. Children and redirects are referenced by integer indices into that array.

| Field | Type | Notes |
|-------|------|-------|
| Node count | [VarInt](./data-types#varint) | Number of nodes that follow. |
| Nodes | Array | Repeats `count` times, see [Node](#node). |
| Root index | [VarInt](./data-types#varint) | Index of the root node. |

### Node

| Field | Type | Notes |
|-------|------|-------|
| Flags | [Byte](./data-types#byte) | See [flags](#flags-byte). |
| Children count | [VarInt](./data-types#varint) | |
| Children indices | [VarInt](./data-types#varint) × count | Indices into the node array. |
| Redirect index | [VarInt](./data-types#varint) (optional) | Present iff `flags & 0x08`. |
| Name | [String](./data-types#string) (optional) | Present for literal/argument nodes. |
| Parser | sub-payload (optional) | Present for argument nodes only — see [Parsers](#parsers). |
| Suggestions identifier | [Identifier](./data-types) (optional) | Present iff `flags & 0x10`. |

#### Flags byte

```text
bits 0..1   Node type:
              0 = root
              1 = literal
              2 = argument
bit  2 (0x04)  is_executable    — node can terminate a command.
bit  3 (0x08)  has_redirect     — node redirects to the node at `redirect index`.
bit  4 (0x10)  has_suggestions  — argument node uses an explicit suggestions provider
                                  identified by `Suggestions identifier`.
bit  5 (0x20)  is_restricted    — node is permission-restricted; the client should
                                  display it dimmed and not suggest it.
bits 6..7      reserved (must be 0).
```

For a **root** node (type 0): no name, no parser, no suggestion id.
For a **literal** node (type 1): `Name` is the literal token (e.g. `"give"`).
For an **argument** node (type 2): `Name` is the parameter name (used in `/help`-style hints), then `Parser` follows, then `Suggestions identifier` if `has_suggestions`.

## Parsers

The argument node's `Parser` sub-payload starts with a VarInt — the parser's numeric ID into the `minecraft:command_argument_type` registry — followed by parser-specific bytes.

The parser registry is fixed for a given protocol version. Parsers in 26.1.2:

| Identifier | Properties |
|------------|------------|
| `brigadier:bool` | none |
| `brigadier:float` | Flags byte (0x01 has_min, 0x02 has_max), optional [Float](./data-types#float) min, optional [Float](./data-types#float) max. |
| `brigadier:double` | Flags byte (0x01 has_min, 0x02 has_max), optional [Double](./data-types#double) min, optional [Double](./data-types#double) max. |
| `brigadier:integer` | Flags byte (0x01 has_min, 0x02 has_max), optional [Int](./data-types#int) min, optional [Int](./data-types#int) max. |
| `brigadier:long` | Flags byte (0x01 has_min, 0x02 has_max), optional [Long](./data-types#long) min, optional [Long](./data-types#long) max. |
| `brigadier:string` | [VarInt](./data-types#varint) mode (0=single word, 1=quotable phrase, 2=greedy phrase). |
| `entity` | Flags byte (0x01 single target, 0x02 only players). |
| `game_profile` | none |
| `block_pos` | none |
| `column_pos` | none |
| `vec3` | none |
| `vec2` | none |
| `block_state` | none |
| `block_predicate` | none |
| `item_stack` | none |
| `item_predicate` | none |
| `color` | none |
| `hex_color` | none |
| `component` | none |
| `style` | none |
| `message` | none |
| `nbt_compound_tag` | none |
| `nbt_tag` | none |
| `nbt_path` | none |
| `objective` | none |
| `objective_criteria` | none |
| `operation` | none |
| `particle` | none |
| `angle` | none |
| `rotation` | none |
| `scoreboard_slot` | none |
| `score_holder` | Flags byte (0x01 allow multiple). |
| `swizzle` | none |
| `team` | none |
| `item_slot` | none |
| `item_slots` | none |
| `resource_location` | none |
| `function` | none |
| `entity_anchor` | none |
| `int_range` | none |
| `float_range` | none |
| `dimension` | none |
| `gamemode` | none |
| `time` | [Int](./data-types#int) min — minimum tick value the parser accepts. |
| `resource_or_tag` | [Identifier](./data-types) — the registry being looked up. |
| `resource_or_tag_key` | [Identifier](./data-types#identifier) — registry. |
| `resource` | [Identifier](./data-types#identifier) — registry. |
| `resource_key` | [Identifier](./data-types#identifier) — registry. |
| `resource_selector` | [Identifier](./data-types#identifier) — registry. |
| `template_mirror` | none |
| `template_rotation` | none |
| `heightmap` | none |
| `loot_table` | none |
| `loot_predicate` | none |
| `loot_modifier` | none |
| `dialog` | none |
| `uuid` | none |

> NOTE: When an argument node uses one of vanilla's *suggestions providers* (rather than the parser's built-in suggestions), `flags & 0x10` is set and `Suggestions identifier` is the provider's identifier. The vanilla providers are `minecraft:ask_server`, `minecraft:all_recipes`, `minecraft:available_sounds`, `minecraft:summonable_entities`. `minecraft:ask_server` instructs the client to call back via `Command Suggestions Request` whenever the user types in that argument.

## Validation

The client validates the graph before installing it: every child/redirect index must point to a node already buildable (no forward dangling references after redirect resolution). A malformed graph terminates the connection.

## Command Suggestions Request (serverbound)

Triggered while the client is editing a command argument backed by `minecraft:ask_server`.

| Field | Type | Notes |
|-------|------|-------|
| Transaction ID | [VarInt](./data-types#varint) | Echoed back by the server. |
| Text | [String](./data-types#string) (≤ 32500) | Current input including the leading `/`. |

## Command Suggestions Response (clientbound)

| Field | Type | Notes |
|-------|------|-------|
| Transaction ID | [VarInt](./data-types#varint) | Matches the request. |
| Range start | [VarInt](./data-types#varint) | Character offset into `Text` where suggestions should replace. |
| Range length | [VarInt](./data-types#varint) | Number of characters to replace. |
| Suggestions count | [VarInt](./data-types#varint) | |
| Suggestions | Array | Repeats `count` times, see below. |

Each suggestion:

| Field | Type | Notes |
|-------|------|-------|
| Match | [String](./data-types#string) | The completion text. |
| Has tooltip | [Boolean](./data-types#boolean) | |
| Tooltip | Component | Only present when `Has tooltip` is true. |

> Source: net/minecraft/network/protocol/game/ClientboundCommandsPacket.java, net/minecraft/network/protocol/game/ClientboundCommandSuggestionsPacket.java, net/minecraft/network/protocol/game/ServerboundCommandSuggestionPacket.java, net/minecraft/commands/synchronization/ArgumentTypeInfos.java, net/minecraft/commands/synchronization/SuggestionProviders.java.
