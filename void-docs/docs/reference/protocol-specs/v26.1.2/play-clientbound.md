# Play — Clientbound Packets

The Play state is the long-lived gameplay protocol entered after the Login (and optional Configuration) state completes. It carries every server-to-client message related to world updates, entities, the player, the inventory, sound, chat, and so on. This page enumerates every clientbound Play packet in registration order — that order, defined by the call sequence of `addPacket(...)` in `GameProtocols.CLIENTBOUND_TEMPLATE`, is the wire packet ID starting at `0x00`.

Common types referenced throughout this page are described in [data-types](./data-types). Heavily structured payloads (chunk data, entity metadata, slots, particles, text components, command graphs, recipe displays, registries, tags) link out to their dedicated reference pages.

## Packet Index

| ID | Name |
|----|------|
| 0x00 | [Bundle Delimiter](#0x00---bundle-delimiter) |
| 0x01 | [Add Entity](#0x01---add-entity) |
| 0x02 | [Animate](#0x02---animate) |
| 0x03 | [Award Stats](#0x03---award-stats) |
| 0x04 | [Block Changed Ack](#0x04---block-changed-ack) |
| 0x05 | [Block Destruction](#0x05---block-destruction) |
| 0x06 | [Block Entity Data](#0x06---block-entity-data) |
| 0x07 | [Block Event](#0x07---block-event) |
| 0x08 | [Block Update](#0x08---block-update) |
| 0x09 | [Boss Event](#0x09---boss-event) |
| 0x0A | [Change Difficulty](#0x0a---change-difficulty) |
| 0x0B | [Chunk Batch Finished](#0x0b---chunk-batch-finished) |
| 0x0C | [Chunk Batch Start](#0x0c---chunk-batch-start) |
| 0x0D | [Chunks Biomes](#0x0d---chunks-biomes) |
| 0x0E | [Clear Titles](#0x0e---clear-titles) |
| 0x0F | [Command Suggestions](#0x0f---command-suggestions) |
| 0x10 | [Commands](#0x10---commands) |
| 0x11 | [Container Close](#0x11---container-close) |
| 0x12 | [Container Set Content](#0x12---container-set-content) |
| 0x13 | [Container Set Data](#0x13---container-set-data) |
| 0x14 | [Container Set Slot](#0x14---container-set-slot) |
| 0x15 | [Cookie Request](#0x15---cookie-request) |
| 0x16 | [Cooldown](#0x16---cooldown) |
| 0x17 | [Custom Chat Completions](#0x17---custom-chat-completions) |
| 0x18 | [Custom Payload](#0x18---custom-payload) |
| 0x19 | [Damage Event](#0x19---damage-event) |
| 0x1A | [Debug Block Value](#0x1a---debug-block-value) |
| 0x1B | [Debug Chunk Value](#0x1b---debug-chunk-value) |
| 0x1C | [Debug Entity Value](#0x1c---debug-entity-value) |
| 0x1D | [Debug Event](#0x1d---debug-event) |
| 0x1E | [Debug Sample](#0x1e---debug-sample) |
| 0x1F | [Delete Chat](#0x1f---delete-chat) |
| 0x20 | [Disconnect](#0x20---disconnect) |
| 0x21 | [Disguised Chat](#0x21---disguised-chat) |
| 0x22 | [Entity Event](#0x22---entity-event) |
| 0x23 | [Entity Position Sync](#0x23---entity-position-sync) |
| 0x24 | [Explode](#0x24---explode) |
| 0x25 | [Forget Level Chunk](#0x25---forget-level-chunk) |
| 0x26 | [Game Event](#0x26---game-event) |
| 0x27 | [Game Rule Values](#0x27---game-rule-values) |
| 0x28 | [Game Test Highlight Pos](#0x28---game-test-highlight-pos) |
| 0x29 | [Mount Screen Open](#0x29---mount-screen-open) |
| 0x2A | [Hurt Animation](#0x2a---hurt-animation) |
| 0x2B | [Initialize Border](#0x2b---initialize-border) |
| 0x2C | [Keep Alive](#0x2c---keep-alive) |
| 0x2D | [Level Chunk With Light](#0x2d---level-chunk-with-light) |
| 0x2E | [Level Event](#0x2e---level-event) |
| 0x2F | [Level Particles](#0x2f---level-particles) |
| 0x30 | [Light Update](#0x30---light-update) |
| 0x31 | [Login](#0x31---login) |
| 0x32 | [Low Disk Space Warning](#0x32---low-disk-space-warning) |
| 0x33 | [Map Item Data](#0x33---map-item-data) |
| 0x34 | [Merchant Offers](#0x34---merchant-offers) |
| 0x35 | [Move Entity Pos](#0x35---move-entity-pos) |
| 0x36 | [Move Entity Pos Rot](#0x36---move-entity-pos-rot) |
| 0x37 | [Move Minecart Along Track](#0x37---move-minecart-along-track) |
| 0x38 | [Move Entity Rot](#0x38---move-entity-rot) |
| 0x39 | [Move Vehicle](#0x39---move-vehicle) |
| 0x3A | [Open Book](#0x3a---open-book) |
| 0x3B | [Open Screen](#0x3b---open-screen) |
| 0x3C | [Open Sign Editor](#0x3c---open-sign-editor) |
| 0x3D | [Ping](#0x3d---ping) |
| 0x3E | [Pong Response](#0x3e---pong-response) |
| 0x3F | [Place Ghost Recipe](#0x3f---place-ghost-recipe) |
| 0x40 | [Player Abilities](#0x40---player-abilities) |
| 0x41 | [Player Chat](#0x41---player-chat) |
| 0x42 | [Player Combat End](#0x42---player-combat-end) |
| 0x43 | [Player Combat Enter](#0x43---player-combat-enter) |
| 0x44 | [Player Combat Kill](#0x44---player-combat-kill) |
| 0x45 | [Player Info Remove](#0x45---player-info-remove) |
| 0x46 | [Player Info Update](#0x46---player-info-update) |
| 0x47 | [Player Look At](#0x47---player-look-at) |
| 0x48 | [Player Position](#0x48---player-position) |
| 0x49 | [Player Rotation](#0x49---player-rotation) |
| 0x4A | [Recipe Book Add](#0x4a---recipe-book-add) |
| 0x4B | [Recipe Book Remove](#0x4b---recipe-book-remove) |
| 0x4C | [Recipe Book Settings](#0x4c---recipe-book-settings) |
| 0x4D | [Remove Entities](#0x4d---remove-entities) |
| 0x4E | [Remove Mob Effect](#0x4e---remove-mob-effect) |
| 0x4F | [Reset Score](#0x4f---reset-score) |
| 0x50 | [Resource Pack Pop](#0x50---resource-pack-pop) |
| 0x51 | [Resource Pack Push](#0x51---resource-pack-push) |
| 0x52 | [Respawn](#0x52---respawn) |
| 0x53 | [Rotate Head](#0x53---rotate-head) |
| 0x54 | [Section Blocks Update](#0x54---section-blocks-update) |
| 0x55 | [Select Advancements Tab](#0x55---select-advancements-tab) |
| 0x56 | [Server Data](#0x56---server-data) |
| 0x57 | [Set Action Bar Text](#0x57---set-action-bar-text) |
| 0x58 | [Set Border Center](#0x58---set-border-center) |
| 0x59 | [Set Border Lerp Size](#0x59---set-border-lerp-size) |
| 0x5A | [Set Border Size](#0x5a---set-border-size) |
| 0x5B | [Set Border Warning Delay](#0x5b---set-border-warning-delay) |
| 0x5C | [Set Border Warning Distance](#0x5c---set-border-warning-distance) |
| 0x5D | [Set Camera](#0x5d---set-camera) |
| 0x5E | [Set Chunk Cache Center](#0x5e---set-chunk-cache-center) |
| 0x5F | [Set Chunk Cache Radius](#0x5f---set-chunk-cache-radius) |
| 0x60 | [Set Cursor Item](#0x60---set-cursor-item) |
| 0x61 | [Set Default Spawn Position](#0x61---set-default-spawn-position) |
| 0x62 | [Set Display Objective](#0x62---set-display-objective) |
| 0x63 | [Set Entity Data](#0x63---set-entity-data) |
| 0x64 | [Set Entity Link](#0x64---set-entity-link) |
| 0x65 | [Set Entity Motion](#0x65---set-entity-motion) |
| 0x66 | [Set Equipment](#0x66---set-equipment) |
| 0x67 | [Set Experience](#0x67---set-experience) |
| 0x68 | [Set Health](#0x68---set-health) |
| 0x69 | [Set Held Slot](#0x69---set-held-slot) |
| 0x6A | [Set Objective](#0x6a---set-objective) |
| 0x6B | [Set Passengers](#0x6b---set-passengers) |
| 0x6C | [Set Player Inventory](#0x6c---set-player-inventory) |
| 0x6D | [Set Player Team](#0x6d---set-player-team) |
| 0x6E | [Set Score](#0x6e---set-score) |
| 0x6F | [Set Simulation Distance](#0x6f---set-simulation-distance) |
| 0x70 | [Set Subtitle Text](#0x70---set-subtitle-text) |
| 0x71 | [Set Time](#0x71---set-time) |
| 0x72 | [Set Title Text](#0x72---set-title-text) |
| 0x73 | [Set Titles Animation](#0x73---set-titles-animation) |
| 0x74 | [Sound Entity](#0x74---sound-entity) |
| 0x75 | [Sound](#0x75---sound) |
| 0x76 | [Start Configuration](#0x76---start-configuration) |
| 0x77 | [Stop Sound](#0x77---stop-sound) |
| 0x78 | [Store Cookie](#0x78---store-cookie) |
| 0x79 | [System Chat](#0x79---system-chat) |
| 0x7A | [Tab List](#0x7a---tab-list) |
| 0x7B | [Tag Query](#0x7b---tag-query) |
| 0x7C | [Take Item Entity](#0x7c---take-item-entity) |
| 0x7D | [Teleport Entity](#0x7d---teleport-entity) |
| 0x7E | [Test Instance Block Status](#0x7e---test-instance-block-status) |
| 0x7F | [Ticking State](#0x7f---ticking-state) |
| 0x80 | [Ticking Step](#0x80---ticking-step) |
| 0x81 | [Transfer](#0x81---transfer) |
| 0x82 | [Update Advancements](#0x82---update-advancements) |
| 0x83 | [Update Attributes](#0x83---update-attributes) |
| 0x84 | [Update Mob Effect](#0x84---update-mob-effect) |
| 0x85 | [Update Recipes](#0x85---update-recipes) |
| 0x86 | [Update Tags](#0x86---update-tags) |
| 0x87 | [Projectile Power](#0x87---projectile-power) |
| 0x88 | [Custom Report Details](#0x88---custom-report-details) |
| 0x89 | [Server Links](#0x89---server-links) |
| 0x8A | [Waypoint](#0x8a---waypoint) |
| 0x8B | [Clear Dialog](#0x8b---clear-dialog) |
| 0x8C | [Show Dialog](#0x8c---show-dialog) |

## 0x00 - Bundle Delimiter

**Packet ID:** `0x00` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| (none) | — | Marker packet; presence delimits a bundle. |

**Semantics.** Sent twice by the server to bracket a group of packets the client should buffer and apply atomically on a single client tick. The opening delimiter starts the bundle, the closing delimiter flushes it. Used to keep multi-packet effects consistent (e.g. spawn entity + initial metadata + velocity).

## 0x01 - Add Entity

**Packet ID:** `0x01` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types#varint) | Server-assigned numeric ID for this entity, used by every subsequent entity-targeted packet. |
| Entity UUID | [UUID](./data-types#uuid) | Stable identifier; randomly generated for newly spawned entities. |
| Type | [VarInt](./data-types#varint) | Registry id into the `minecraft:entity_type` registry. |
| X / Y / Z | [Double](./data-types#double) × 3 | Spawn position in absolute world coordinates. |
| Velocity X / Y / Z | [Short](./data-types#short) × 3 | Initial velocity, encoded as `clamp(value, -3.9, 3.9) * 8000`. |
| Pitch | [Angle](./data-types#angle) | Packed degrees (`degrees * 256/360`). |
| Yaw | [Angle](./data-types#angle) | Packed degrees. |
| Head Yaw | [Angle](./data-types#angle) | Packed degrees; relevant for mobs with an independent head. |
| Data | [VarInt](./data-types#varint) | Object-specific data: e.g. block state id for falling blocks, shooter entity id for projectiles. Meaning depends on Type. |

**Semantics.** Sent when a non-player entity becomes visible to the client. Players are introduced via [Player Info Update](#0x46---player-info-update) and entered into the world by their entry there. Velocity is sent in the `Vec3.LP_STREAM_CODEC` form (three shorts).

## 0x02 - Animate

**Packet ID:** `0x02` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types#varint) | Target entity. |
| Animation | [Unsigned Byte](./data-types#unsigned-byte) | `0` swing main hand, `2` wake up, `3` swing off hand, `4` critical hit, `5` magic critical hit. |

**Semantics.** Plays a one-shot first-person/third-person animation on the target entity.

## 0x03 - Award Stats

**Packet ID:** `0x03` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Count | [VarInt](./data-types#varint) | Number of statistic entries. |
| Entries | Array | Repeated `count` times, each `(Stat Category VarInt, Stat Id VarInt, Value VarInt)` per the `Stat.STREAM_CODEC` encoding. |

**Semantics.** Delivers updated statistics totals for the player; usually sent in response to a `client_command` `request_stats`.

## 0x04 - Block Changed Ack

**Packet ID:** `0x04` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Sequence | [VarInt](./data-types#varint) | Mirrors the `sequence` field of a player block action; lets the client retire predicted block changes up to this id. |

**Semantics.** Acknowledges block-modification predictions sent by the client and allows it to release client-side rollback state.

## 0x05 - Block Destruction

**Packet ID:** `0x05` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types#varint) | Identifies the breaking entity (so its progress can be replaced or cleared). |
| Position | [BlockPos](./data-types#blockpos) | Block being broken. |
| Progress | [Unsigned Byte](./data-types#unsigned-byte) | `0`–`9` show the breaking overlay; values `>= 10` clear the overlay. |

**Semantics.** Drives the cracked-block overlay other players see while one player mines a block.

## 0x06 - Block Entity Data

**Packet ID:** `0x06` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Position | [BlockPos](./data-types#blockpos) | Location of the block entity. |
| Type | [VarInt](./data-types#varint) | Registry id into `minecraft:block_entity_type`. |
| NBT | [NBT Compound](./data-types#nbt) | Block-entity-specific update tag (network-trusted compound). |

**Semantics.** Updates the data of an existing block entity (sign text, beacon settings, etc.). The NBT layout depends on the type; see the corresponding block entity.

## 0x07 - Block Event

**Packet ID:** `0x07` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Position | [BlockPos](./data-types#blockpos) | Block whose event is being fired. |
| Action Id | [Unsigned Byte](./data-types#unsigned-byte) | Block-specific action identifier (e.g. note block instrument, chest open). |
| Action Param | [Unsigned Byte](./data-types#unsigned-byte) | Block-specific parameter (e.g. note pitch, viewer count). |
| Block Type | [VarInt](./data-types#varint) | Registry id into `minecraft:block`; a sanity check against the position. |

**Semantics.** Fires a transient block-driven action visible to nearby clients (note block ping, chest lid animation, piston extension, beacon beam). Meaning of action/param is per-block.

## 0x08 - Block Update

**Packet ID:** `0x08` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Position | [BlockPos](./data-types#blockpos) | Block to update. |
| Block State Id | [VarInt](./data-types#varint) | Index into the global block-state palette. |

**Semantics.** Replaces a single block. For multiple changes within one chunk section, see [Section Blocks Update](#0x54---section-blocks-update); for whole chunks, [Level Chunk With Light](#0x2d---level-chunk-with-light).

## 0x09 - Boss Event

**Packet ID:** `0x09` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Boss Bar Id | [UUID](./data-types#uuid) | Identifies the bar. |
| Operation | [Enum](./data-types#enum) ([Byte](./data-types#byte)) | `0` ADD, `1` REMOVE, `2` UPDATE_PROGRESS, `3` UPDATE_NAME, `4` UPDATE_STYLE, `5` UPDATE_PROPERTIES. |
| Operation Body | Variant | Fields below depend on operation. |

ADD body:

| Field | Type | Notes |
|-------|------|-------|
| Name | [Text Component](./text-component) | Bar title. |
| Progress | [Float](./data-types#float) | `0.0`–`1.0` filled fraction. |
| Color | [Enum](./data-types#enum) ([VarInt](./data-types#varint)) | `PINK`, `BLUE`, `RED`, `GREEN`, `YELLOW`, `PURPLE`, `WHITE`. |
| Overlay | [Enum](./data-types#enum) ([VarInt](./data-types#varint)) | `PROGRESS`, `NOTCHED_6`, `NOTCHED_10`, `NOTCHED_12`, `NOTCHED_20`. |
| Flags | [Unsigned Byte](./data-types#unsigned-byte) | Bitfield: `0x01` darken sky, `0x02` boss music, `0x04` create world fog. |

REMOVE body: empty.
UPDATE_PROGRESS body: `Progress` (Float).
UPDATE_NAME body: `Name` (Text Component).
UPDATE_STYLE body: `Color` (Enum), `Overlay` (Enum).
UPDATE_PROPERTIES body: `Flags` (Unsigned Byte) as in ADD.

**Semantics.** Manages a named boss bar visible to the player.

## 0x0A - Change Difficulty

**Packet ID:** `0x0A` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Difficulty | [Enum](./data-types#enum) ([Unsigned Byte](./data-types#unsigned-byte)) | `0` PEACEFUL, `1` EASY, `2` NORMAL, `3` HARD. |
| Locked | [Bool](./data-types#bool) | If true, the client cannot change the difficulty. |

**Semantics.** Synchronises the world difficulty and its lock state with the client.

## 0x0B - Chunk Batch Finished

**Packet ID:** `0x0B` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Batch Size | [VarInt](./data-types#varint) | Number of chunks delivered in the just-finished batch. |

**Semantics.** Marks the end of a batch of [Level Chunk With Light](#0x2d---level-chunk-with-light) packets. The client uses this together with [Chunk Batch Start](#0x0c---chunk-batch-start) to estimate per-chunk decode cost and respond with a `chunk_batch_received` rate.

## 0x0C - Chunk Batch Start

**Packet ID:** `0x0C` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| (none) | — | Marker packet. |

**Semantics.** Marks the beginning of a chunk batch; allows the client to time its decode.

## 0x0D - Chunks Biomes

**Packet ID:** `0x0D` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Count | [VarInt](./data-types#varint) | Number of chunks in the update. |
| Chunks | Array | Repeated `count` times: `(Chunk X Int, Chunk Z Int, Data Byte Array)` where Data is the biome-only chunk payload (length-prefixed by VarInt, max 2 MiB). |

**Semantics.** Refreshes only the biome layers of already-loaded chunks without replacing block data. See [chunks](./chunks) for the biome palette layout.

## 0x0E - Clear Titles

**Packet ID:** `0x0E` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Reset | [Bool](./data-types#bool) | If true, also resets fade-in/stay/fade-out timings to defaults. |

**Semantics.** Hides any displayed title and subtitle; optionally restores default animation timings.

## 0x0F - Command Suggestions

**Packet ID:** `0x0F` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Transaction Id | [VarInt](./data-types#varint) | Echoes the id from the corresponding `command_suggestion` request. |
| Start | [VarInt](./data-types#varint) | Index in the command string where the replacement begins. |
| Length | [VarInt](./data-types#varint) | Number of characters being replaced. |
| Count | [VarInt](./data-types#varint) | Number of suggestion entries. |
| Entries | Array | Repeated `count` times: `(Text String, Has Tooltip Bool, Tooltip Text Component?)`. |

**Semantics.** Server-side completion results for a player's tab-complete request.

## 0x10 - Commands

**Packet ID:** `0x10` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Nodes | Array | Brigadier command graph; each entry encodes flags, children indices, optional redirect, and per-type payload. See [commands](./commands). |
| Root Index | [VarInt](./data-types#varint) | Index of the root node in `Nodes`. |

**Semantics.** Synchronises the executable command graph available to the player. The client builds its parser/auto-completer from this.

## 0x11 - Container Close

**Packet ID:** `0x11` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Container Id | [VarInt](./data-types#varint) | Id of the container to close (read via `readContainerId`). |

**Semantics.** Forces the client to close a server-opened container window.

## 0x12 - Container Set Content

**Packet ID:** `0x12` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Container Id | [VarInt](./data-types#varint) | Container window id. |
| State Id | [VarInt](./data-types#varint) | Server snapshot id; echoed by the next [Container Click](./play-serverbound#container-click). |
| Items | Array of [Slot](./slot) | Full inventory contents in slot order. |
| Carried Item | [Slot](./slot) | Item currently held on the cursor. |

**Semantics.** Sends a complete snapshot of an opened container; supersedes any prior partial updates.

## 0x13 - Container Set Data

**Packet ID:** `0x13` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Container Id | [VarInt](./data-types#varint) | Container window id. |
| Property Id | [Short](./data-types#short) | Container-specific property index (e.g. furnace progress). |
| Value | [Short](./data-types#short) | New value of that property. |

**Semantics.** Updates a single integer property of a container (furnace burn time, brewing fuel, beacon level, etc.).

## 0x14 - Container Set Slot

**Packet ID:** `0x14` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Container Id | [VarInt](./data-types#varint) | Container window id; `-1` for the carried item, `-2` for the player's inventory. |
| State Id | [VarInt](./data-types#varint) | Server snapshot id. |
| Slot | [Short](./data-types#short) | Index of the slot to overwrite. |
| Item | [Slot](./slot) | New stack contents. |

**Semantics.** Updates a single inventory slot.

## 0x15 - Cookie Request

**Packet ID:** `0x15` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Key | [Identifier](./data-types#identifier) | Cookie key the server wants the client to return. |

**Semantics.** Asks the client to reply with a `cookie_response` containing the value previously stored under `key` (or empty if absent).

## 0x16 - Cooldown

**Packet ID:** `0x16` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Cooldown Group | [Identifier](./data-types#identifier) | Cooldown bucket (typically an item id). |
| Duration | [VarInt](./data-types#varint) | Cooldown ticks remaining; `0` clears the cooldown. |

**Semantics.** Greys out items sharing the cooldown group for the given number of ticks.

## 0x17 - Custom Chat Completions

**Packet ID:** `0x17` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Action | [Enum](./data-types#enum) ([VarInt](./data-types#varint)) | `0` ADD, `1` REMOVE, `2` SET. |
| Entries | Array of [String](./data-types#string) | Names to add/remove/replace in the chat completion list. |

**Semantics.** Maintains the server-driven completion suggestions shown in the chat UI (typically online player names plus custom tokens).

## 0x18 - Custom Payload

**Packet ID:** `0x18` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Channel | [Identifier](./data-types#identifier) | Plugin channel identifier. |
| Data | [Byte Array](./data-types#byte-array) | Channel-specific payload; total packet size capped at 1 MiB. The vanilla `minecraft:brand` payload uses a single UTF-8 string; otherwise the payload is opaque. |

**Semantics.** Carries plugin-defined messages from server to client.

## 0x19 - Damage Event

**Packet ID:** `0x19` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Entity Id | [VarInt](./data-types#varint) | Entity that was damaged. |
| Source Type Id | [VarInt](./data-types#varint) | Registry id into `minecraft:damage_type`. |
| Source Cause Id | [VarInt](./data-types#varint) | Direct/indirect attacker id encoded as `id + 1`; `0` means none. |
| Source Direct Id | [VarInt](./data-types#varint) | Projectile / direct entity id encoded the same way. |
| Has Source Position | [Bool](./data-types#bool) | If true, position fields follow. |
| Source Position | [Double](./data-types#double) × 3 | Optional. Origin point of the damage. |

**Semantics.** Notifies clients of a damage event so they can play the correct hurt animation, sounds and particles.

## 0x1A - Debug Block Value

**Packet ID:** `0x1A` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Block Position | [BlockPos](./data-types#blockpos) | Subject block. |
| Update | Tagged Update | `DebugSubscription.Update` payload: subscription type id followed by type-specific data. |

**Semantics.** Pushes a block-scoped debug data update to the F3 debug overlay; only sent to clients that subscribed via the debug subscription request.

## 0x1B - Debug Chunk Value

**Packet ID:** `0x1B` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Chunk Position | [ChunkPos](./data-types#chunkpos) | Subject chunk. |
| Update | Tagged Update | See [Debug Block Value](#0x1a---debug-block-value). |

**Semantics.** Chunk-scoped debug data update for the debug overlay.

## 0x1C - Debug Entity Value

**Packet ID:** `0x1C` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Entity Id | [VarInt](./data-types#varint) | Subject entity. |
| Update | Tagged Update | Subscription-specific payload. |

**Semantics.** Entity-scoped debug data update.

## 0x1D - Debug Event

**Packet ID:** `0x1D` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Event | Tagged Event | `DebugSubscription.Event` payload: subscription type id followed by type-specific data. |

**Semantics.** Generic non-targeted debug event for subscribed debug overlays.

## 0x1E - Debug Sample

**Packet ID:** `0x1E` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Sample | [Long](./data-types#long) Array | Sequence of `Long` values; the meaning depends on the sample type. |
| Sample Type | [Enum](./data-types#enum) ([VarInt](./data-types#varint)) | `RemoteDebugSampleType`, currently `TICK_TIME`. |

**Semantics.** Sends a batched performance sample (e.g. tick durations) to the debug client.

## 0x1F - Delete Chat

**Packet ID:** `0x1F` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Message Signature | Packed Signature | Either a 256-byte signature or a VarInt back-reference into the rolling signature cache (`MessageSignature.Packed`). |

**Semantics.** Removes a previously displayed chat message identified by its signature reference.

## 0x20 - Disconnect

**Packet ID:** `0x20` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Reason | [Text Component](./text-component) | Disconnect message; serialized via the trusted context-free codec. |

**Semantics.** Closes the connection and shows the reason on the client's disconnection screen.

## 0x21 - Disguised Chat

**Packet ID:** `0x21` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Message | [Text Component](./text-component) | Already-formatted message body. |
| Chat Type | Bound Chat Type | `(Chat Type Holder, Sender Name Text Component, Optional Target Name Text Component)` — see [chat](./chat). |

**Semantics.** Server-side chat message that should appear to come from a player but does not carry any signature; used by `/say`, `/me` and similar.

## 0x22 - Entity Event

**Packet ID:** `0x22` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Entity Id | [Int](./data-types#int) | Target entity (note: 32-bit int, not VarInt). |
| Event Id | [Byte](./data-types#byte) | Entity-status code (e.g. `2` hurt animation, `3` death sound). |

**Semantics.** Fires a status event on an entity; the meaning of `Event Id` is per entity class.

## 0x23 - Entity Position Sync

**Packet ID:** `0x23` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Entity Id | [VarInt](./data-types#varint) | Target entity. |
| Position | [Double](./data-types#double) × 3 | Absolute X, Y, Z. |
| Delta | [Double](./data-types#double) × 3 | Velocity X, Y, Z. |
| Yaw | [Float](./data-types#float) | Degrees. |
| Pitch | [Float](./data-types#float) | Degrees. |
| On Ground | [Bool](./data-types#bool) | Whether the entity is grounded. |

**Semantics.** Authoritative absolute-position sync for an entity (used to correct accumulated drift from delta-only updates). Uses the `PositionMoveRotation` codec.

## 0x24 - Explode

**Packet ID:** `0x24` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Center | [Double](./data-types#double) × 3 | Centre of the explosion. |
| Radius | [Float](./data-types#float) | Effective explosion radius. |
| Block Count | [Int](./data-types#int) | Number of blocks broken (used for particle weighting). |
| Has Player Knockback | [Bool](./data-types#bool) | Followed by `Vec3` (3 doubles) if true. |
| Explosion Particle | [Particle](./particle) | Particle used at the centre. |
| Explosion Sound | Sound Holder | Inline `SoundEvent` (registry id, optional override). |
| Block Particles | Weighted List of Explosion [Particle](./particle) Info | Particles spawned per broken block, with weights. |

**Semantics.** Notifies the client of an explosion at a point; drives sound, screen shake, particles and player knockback.

## 0x25 - Forget Level Chunk

**Packet ID:** `0x25` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Chunk Position | [ChunkPos](./data-types#chunkpos) | Encoded as a single `Long` (`x` low 32 bits, `z` high 32 bits). |

**Semantics.** Tells the client to unload a previously sent chunk.

## 0x26 - Game Event

**Packet ID:** `0x26` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Event | [Unsigned Byte](./data-types#unsigned-byte) | Event id. Notable values: `0` no respawn block, `1` end raining, `2` begin raining, `3` change game mode, `4` win game, `5` demo event, `6` arrow hit player, `7` rain level change, `8` thunder level change, `9` puffer fish sting, `10` elder guardian appearance, `11` immediate respawn, `13` start waiting for chunks. |
| Param | [Float](./data-types#float) | Event-specific parameter (e.g. game mode index). |

**Semantics.** Generic, world-wide gameplay event for the receiving player.

## 0x27 - Game Rule Values

**Packet ID:** `0x27` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Values | Map of `(GameRule ResourceKey, [String](./data-types#string))` | Each entry is a registry resource key for a gamerule and its current value as a string. |

**Semantics.** Sends the current value of every gamerule to the client (e.g. for the gamerule UI).

## 0x28 - Game Test Highlight Pos

**Packet ID:** `0x28` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Absolute Position | [BlockPos](./data-types#blockpos) | World-space position. |
| Relative Position | [BlockPos](./data-types#blockpos) | Test-instance-local position. |

**Semantics.** Highlights a position in the world during gametest sessions.

## 0x29 - Mount Screen Open

**Packet ID:** `0x29` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Container Id | [VarInt](./data-types#varint) | Window id. |
| Inventory Columns | [VarInt](./data-types#varint) | Width of the mount inventory in columns. |
| Entity Id | [Int](./data-types#int) | Mount entity (note: 32-bit int). |

**Semantics.** Opens the mount inventory screen (horse, llama, donkey...) for the given mount.

## 0x2A - Hurt Animation

**Packet ID:** `0x2A` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Entity Id | [VarInt](./data-types#varint) | Hurt entity. |
| Yaw | [Float](./data-types#float) | Direction (degrees) the damage came from, used to tilt the model. |

**Semantics.** Plays the directional knockback/hurt animation on the entity.

## 0x2B - Initialize Border

**Packet ID:** `0x2B` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Center X | [Double](./data-types#double) | World border centre X. |
| Center Z | [Double](./data-types#double) | Centre Z. |
| Old Diameter | [Double](./data-types#double) | Current diameter. |
| New Diameter | [Double](./data-types#double) | Target diameter. |
| Lerp Time (ms) | [VarLong](./data-types#varlong) | Milliseconds over which to interpolate to `New Diameter`. |
| Portal Boundary | [VarInt](./data-types#varint) | Absolute maximum coordinate (typically 29999984). |
| Warning Blocks | [VarInt](./data-types#varint) | Distance from border at which the red warning shader appears. |
| Warning Time | [VarInt](./data-types#varint) | Seconds-of-shrink threshold for the warning. |

**Semantics.** Sets the full state of the world border in one packet on player join or respawn.

## 0x2C - Keep Alive

**Packet ID:** `0x2C` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Id | [Long](./data-types#long) | Random ping id; the client must echo it via `keep_alive`. |

**Semantics.** Liveness probe. Must be answered within the server timeout (default 30 s) or the player is disconnected.

## 0x2D - Level Chunk With Light

**Packet ID:** `0x2D` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Chunk X | [Int](./data-types#int) | |
| Chunk Z | [Int](./data-types#int) | |
| Chunk Data | Chunk Packet Data | Heightmaps NBT, block-state palette payload, block entities — see [chunks](./chunks). |
| Light Data | Light Update Packet Data | Sky/block light masks and arrays — see [chunks](./chunks). |

**Semantics.** Delivers a full chunk plus its lighting in a single packet. Sent in batches bracketed by [Chunk Batch Start](#0x0c---chunk-batch-start) / [Chunk Batch Finished](#0x0b---chunk-batch-finished).

## 0x2E - Level Event

**Packet ID:** `0x2E` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Event Id | [Int](./data-types#int) | Numeric effect id (block break sound, smoke direction, etc.). |
| Position | [BlockPos](./data-types#blockpos) | Effect origin. |
| Data | [Int](./data-types#int) | Event-specific parameter. |
| Global | [Bool](./data-types#bool) | If true, plays for all players regardless of distance. |

**Semantics.** Plays a one-shot world effect (sound + particles) at a position.

## 0x2F - Level Particles

**Packet ID:** `0x2F` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Override Limiter | [Bool](./data-types#bool) | If true, ignores the client particle-distance setting. |
| Always Show | [Bool](./data-types#bool) | If true, render even with reduced particle settings. |
| X / Y / Z | [Double](./data-types#double) × 3 | Spawn position. |
| Offset X / Y / Z | [Float](./data-types#float) × 3 | Random spread on each axis. |
| Max Speed | [Float](./data-types#float) | Velocity scalar. |
| Count | [Int](./data-types#int) | Number of particles to spawn. |
| Particle | [Particle](./particle) | Particle type plus per-type extra fields. |

**Semantics.** Spawns particles at a world position. Particle-specific fields follow the type id (block/item/dust/etc.).

## 0x30 - Light Update

**Packet ID:** `0x30` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Chunk X | [VarInt](./data-types#varint) | |
| Chunk Z | [VarInt](./data-types#varint) | |
| Light Data | Light Update Packet Data | Sky/block light arrays + masks — see [chunks](./chunks). |

**Semantics.** Updates only the light arrays of an already-loaded chunk (e.g. after block changes that altered light propagation).

## 0x31 - Login

**Packet ID:** `0x31` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Player Id | [Int](./data-types#int) | Server-side entity id of the player. |
| Hardcore | [Bool](./data-types#bool) | Hardcore-mode flag. |
| Dimensions | Array of [Identifier](./data-types#identifier) | Names of every world (`ResourceKey<Level>`) the player may visit. |
| Max Players | [VarInt](./data-types#varint) | Soft cap (display only). |
| View Distance | [VarInt](./data-types#varint) | Render distance in chunks. |
| Simulation Distance | [VarInt](./data-types#varint) | Simulation radius in chunks. |
| Reduced Debug Info | [Bool](./data-types#bool) | If true, hides certain F3 fields. |
| Show Death Screen | [Bool](./data-types#bool) | If false, players respawn immediately. |
| Limited Crafting | [Bool](./data-types#bool) | If true, recipe book unlocks gate crafting. |
| Common Spawn Info | Spawn Info | `(Dimension Type Id VarInt, Dimension Name Identifier, Hashed Seed Long, Game Mode Byte, Previous Game Mode Byte, Is Debug Bool, Is Flat Bool, Death Location? (BlockPos+Identifier), Portal Cooldown VarInt, Sea Level VarInt)`. |
| Enforces Secure Chat | [Bool](./data-types#bool) | If true, the client requires signed chat. |

**Semantics.** Initial world-info packet entering the Play state. Carries enough to instantiate the player entity and the local world.

## 0x32 - Low Disk Space Warning

**Packet ID:** `0x32` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| (none) | — | Marker packet. |

**Semantics.** Singleton warning that the local integrated server is running low on disk space; for vanilla single-player only.

## 0x33 - Map Item Data

**Packet ID:** `0x33` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Map Id | [VarInt](./data-types#varint) | Identifies which filled map to update. |
| Scale | [Byte](./data-types#byte) | Map scale (`0`–`4`). |
| Locked | [Bool](./data-types#bool) | True if locked with a glass pane. |
| Has Decorations | [Bool](./data-types#bool) | If true, a list of decorations follows. |
| Decorations | Array of MapDecoration | Optional. Each `(Type VarInt, X Byte, Z Byte, Rotation Byte, Optional Display Name Text Component)`. |
| Has Color Patch | [Bool](./data-types#bool) | If true, a patch follows. |
| Color Patch | MapPatch | Optional. `(Width Byte, Height Byte, X Byte, Z Byte, Pixels Byte Array)`. |

**Semantics.** Updates the rendered image / decorations of a filled map item.

## 0x34 - Merchant Offers

**Packet ID:** `0x34` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Container Id | [VarInt](./data-types#varint) | Active trading window. |
| Trades | Array of MerchantOffer | Each offer encodes input items, output, uses, max uses, xp, special price, price multiplier and demand. |
| Villager Level | [VarInt](./data-types#varint) | `1`–`5` (Novice…Master). |
| Villager Xp | [VarInt](./data-types#varint) | Total experience. |
| Show Progress | [Bool](./data-types#bool) | Whether to show the level-up bar. |
| Can Restock | [Bool](./data-types#bool) | True for villagers, false for wandering traders. |

**Semantics.** Populates a villager / wandering-trader trade GUI.

## 0x35 - Move Entity Pos

**Packet ID:** `0x35` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Entity Id | [VarInt](./data-types#varint) | Target entity. |
| Delta X | [Short](./data-types#short) | `(currentX*4096) - (prevX*4096)`. |
| Delta Y | [Short](./data-types#short) | Same for Y. |
| Delta Z | [Short](./data-types#short) | Same for Z. |
| On Ground | [Bool](./data-types#bool) | Whether the entity touches the ground. |

**Semantics.** Position-only delta update. Each delta is limited to ±8 blocks; for larger movement, the server sends [Teleport Entity](#0x7d---teleport-entity).

## 0x36 - Move Entity Pos Rot

**Packet ID:** `0x36` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Entity Id | [VarInt](./data-types#varint) | Target. |
| Delta X / Y / Z | [Short](./data-types#short) × 3 | Same encoding as [Move Entity Pos](#0x35---move-entity-pos). |
| Yaw | [Angle](./data-types#angle) | Packed degrees. |
| Pitch | [Angle](./data-types#angle) | Packed degrees. |
| On Ground | [Bool](./data-types#bool) | |

**Semantics.** Combined position-and-rotation delta update.

## 0x37 - Move Minecart Along Track

**Packet ID:** `0x37` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Entity Id | [VarInt](./data-types#varint) | Minecart. |
| Lerp Steps | Array of MinecartStep | Each `(Position Vec3, Movement Vec3, Yaw Float, Pitch Float, Weight Float)`. |

**Semantics.** Server-driven smooth interpolation steps for the new minecart movement model.

## 0x38 - Move Entity Rot

**Packet ID:** `0x38` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Entity Id | [VarInt](./data-types#varint) | Target. |
| Yaw | [Angle](./data-types#angle) | Packed degrees. |
| Pitch | [Angle](./data-types#angle) | Packed degrees. |
| On Ground | [Bool](./data-types#bool) | |

**Semantics.** Rotation-only update for an entity that has not moved.

## 0x39 - Move Vehicle

**Packet ID:** `0x39` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Position | [Double](./data-types#double) × 3 | Absolute X, Y, Z. |
| Yaw | [Float](./data-types#float) | Degrees. |
| Pitch | [Float](./data-types#float) | Degrees. |

**Semantics.** Server-corrected position/rotation for the vehicle the player is riding.

## 0x3A - Open Book

**Packet ID:** `0x3A` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Hand | [Enum](./data-types#enum) ([VarInt](./data-types#varint)) | `0` MAIN_HAND, `1` OFF_HAND. |

**Semantics.** Opens the written-book GUI for the book held in the given hand.

## 0x3B - Open Screen

**Packet ID:** `0x3B` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Container Id | [VarInt](./data-types#varint) | Newly assigned window id. |
| Type | [VarInt](./data-types#varint) | Registry id into `minecraft:menu` (chest, anvil, …). |
| Title | [Text Component](./text-component) | Window title. |

**Semantics.** Tells the client to open a container window of the given type.

## 0x3C - Open Sign Editor

**Packet ID:** `0x3C` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Position | [BlockPos](./data-types#blockpos) | Sign block. |
| Is Front Text | [Bool](./data-types#bool) | True for front side, false for back. |

**Semantics.** Opens the sign-editing UI for the given sign side.

## 0x3D - Ping

**Packet ID:** `0x3D` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Id | [Int](./data-types#int) | Echoed back via `pong` (note: 32-bit int, distinct from Keep Alive). |

**Semantics.** Generic round-trip probe; commonly used by datapacks/plugins to measure latency.

## 0x3E - Pong Response

**Packet ID:** `0x3E` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Time | [Long](./data-types#long) | Server-supplied timestamp echoed in response to `ping_request`. |

**Semantics.** Server-side reply to the client's status-style `ping_request`.

## 0x3F - Place Ghost Recipe

**Packet ID:** `0x3F` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Container Id | [VarInt](./data-types#varint) | Crafting window. |
| Recipe Display | RecipeDisplay | Display data for the recipe (slot images and result) — see [recipe display](./recipes). |

**Semantics.** Shows the ghost-item preview of a recipe in the crafting grid after the player clicks it in the recipe book.

## 0x40 - Player Abilities

**Packet ID:** `0x40` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Flags | [Byte](./data-types#byte) | Bitfield: `0x01` invulnerable, `0x02` flying, `0x04` may fly, `0x08` instant break (creative). |
| Flying Speed | [Float](./data-types#float) | Speed multiplier while flying. |
| Walking Speed | [Float](./data-types#float) | Field-of-view modifier base speed. |

**Semantics.** Updates the player's god-mode/flight abilities and speed multipliers.

## 0x41 - Player Chat

**Packet ID:** `0x41` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Global Index | [VarInt](./data-types#varint) | Strictly increasing message index for the receiving client. |
| Sender | [UUID](./data-types#uuid) | Sender's player UUID. |
| Index | [VarInt](./data-types#varint) | Per-sender chat index for ordering and validation. |
| Has Signature | [Bool](./data-types#bool) | If true, a 256-byte signature follows. |
| Signature | [Byte](./data-types#byte)[256] | Optional. |
| Body | SignedMessageBody.Packed | `(Content String, Timestamp Long, Salt Long, Last-Seen Acks)` — see [chat](./chat). |
| Has Unsigned Content | [Bool](./data-types#bool) | If true, an alternative formatted form follows. |
| Unsigned Content | [Text Component](./text-component) | Optional. |
| Filter Mask | FilterMask | `(Type Enum, Bitset)` indicating which characters were redacted. |
| Chat Type | Bound Chat Type | Decoration parameters — see [chat](./chat). |

**Semantics.** Carries a signed chat message and the metadata needed to verify it.

## 0x42 - Player Combat End

**Packet ID:** `0x42` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Duration | [VarInt](./data-types#varint) | Combat duration in ticks. |

**Semantics.** Signals end of a combat episode (used by combat tracking only).

## 0x43 - Player Combat Enter

**Packet ID:** `0x43` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| (none) | — | Marker packet. |

**Semantics.** Signals start of a combat episode.

## 0x44 - Player Combat Kill

**Packet ID:** `0x44` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Player Id | [VarInt](./data-types#varint) | Killed player. |
| Message | [Text Component](./text-component) | Death message. |

**Semantics.** Drives the death screen overlay on the receiving client.

## 0x45 - Player Info Remove

**Packet ID:** `0x45` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Profile Ids | Array of [UUID](./data-types#uuid) | Players to remove from the tab list and from the local player registry. |

**Semantics.** Removes player entries from the client's player-list table.

## 0x46 - Player Info Update

**Packet ID:** `0x46` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Actions | Bitset ([Byte](./data-types#byte)) | Combination of `ADD_PLAYER`, `INITIALIZE_CHAT`, `UPDATE_GAME_MODE`, `UPDATE_LISTED`, `UPDATE_LATENCY`, `UPDATE_DISPLAY_NAME`, `UPDATE_LIST_PRIORITY`, `UPDATE_HAT`. |
| Entries | Array of Entry | For each entry: `(UUID)` followed, in `Actions` enum order, by the fields each chosen action requires. |

Per-action payload:

- `ADD_PLAYER`: `(Name String, Properties Array of (Name, Value, Optional Signature))`.
- `INITIALIZE_CHAT`: optional `(Session Id UUID, Public Key Expiry Long, Public Key Bytes, Key Signature Bytes)`.
- `UPDATE_GAME_MODE`: `(Game Mode VarInt)`.
- `UPDATE_LISTED`: `(Listed Bool)`.
- `UPDATE_LATENCY`: `(Latency VarInt ms)`.
- `UPDATE_DISPLAY_NAME`: optional `(Display Name Text Component)`.
- `UPDATE_LIST_PRIORITY`: `(Priority VarInt)`.
- `UPDATE_HAT`: `(Show Hat Bool)`.

**Semantics.** Atomically adds or updates one or more player-list entries.

## 0x47 - Player Look At

**Packet ID:** `0x47` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| From Anchor | [Enum](./data-types#enum) ([VarInt](./data-types#varint)) | `0` FEET, `1` EYES — origin point on the player. |
| Target X / Y / Z | [Double](./data-types#double) × 3 | Target position. |
| Is Entity | [Bool](./data-types#bool) | If true, two more fields follow. |
| Target Entity | [VarInt](./data-types#varint) | Optional. Entity to look at instead of the position. |
| To Anchor | [Enum](./data-types#enum) ([VarInt](./data-types#varint)) | Optional. Anchor on the target entity. |

**Semantics.** Forces the player camera to face a position or entity (the `/teleport ... facing` command).

## 0x48 - Player Position

**Packet ID:** `0x48` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Teleport Id | [VarInt](./data-types#varint) | Echoed by the client's `accept_teleportation`. |
| Change | PositionMoveRotation | `(Position Vec3, Delta Vec3, Yaw Float, Pitch Float)`. |
| Relatives | Bitset of `Relative` | Which fields are deltas instead of absolutes (`X`, `Y`, `Z`, `Y_ROT`, `X_ROT`, `DELTA_X`, `DELTA_Y`, `DELTA_Z`, `ROTATE_DELTA`). |

**Semantics.** Authoritatively sets the player's position/rotation. The client must reply with `accept_teleportation` carrying the same id.

## 0x49 - Player Rotation

**Packet ID:** `0x49` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Yaw | [Float](./data-types#float) | Degrees. |
| Relative Yaw | [Bool](./data-types#bool) | If true, yaw is added to the current value. |
| Pitch | [Float](./data-types#float) | Degrees. |
| Relative Pitch | [Bool](./data-types#bool) | If true, pitch is added. |

**Semantics.** Forces a rotation change on the player (relative or absolute per axis).

## 0x4A - Recipe Book Add

**Packet ID:** `0x4A` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Entries | Array of Entry | Each `(RecipeDisplayEntry contents, Flags Byte)` where `0x01` shows a notification and `0x02` highlights the entry. |
| Replace | [Bool](./data-types#bool) | If true, the new list replaces the existing recipe book contents. |

**Semantics.** Adds entries to (or replaces the contents of) the client recipe book — see [recipe display](./recipes).

## 0x4B - Recipe Book Remove

**Packet ID:** `0x4B` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Recipe Ids | Array of RecipeDisplayId | Recipes to remove from the book. |

**Semantics.** Removes recipes from the client recipe book.

## 0x4C - Recipe Book Settings

**Packet ID:** `0x4C` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Book Settings | RecipeBookSettings | One `(Open Bool, Filter Bool)` pair per book type (Crafting, Furnace, Blast Furnace, Smoker). |

**Semantics.** Synchronises per-book open/filter state with the client.

## 0x4D - Remove Entities

**Packet ID:** `0x4D` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Entity Ids | [VarInt](./data-types#varint) Array (`readIntIdList`) | Entities to despawn from the client world. |

**Semantics.** Removes one or more entities from the client.

## 0x4E - Remove Mob Effect

**Packet ID:** `0x4E` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Entity Id | [VarInt](./data-types#varint) | Affected entity. |
| Effect | [VarInt](./data-types#varint) | Registry id into `minecraft:mob_effect`. |

**Semantics.** Cancels a mob effect on an entity (e.g. milk cured Poison).

## 0x4F - Reset Score

**Packet ID:** `0x4F` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Owner | [String](./data-types#string) | Score owner (player name or fake-player). |
| Has Objective | [Bool](./data-types#bool) | If true, an objective name follows. |
| Objective | [String](./data-types#string) | Optional. Specific objective to reset; if absent, all of the owner's scores are reset. |

**Semantics.** Removes one or all sidebar/objective scores for an owner.

## 0x50 - Resource Pack Pop

**Packet ID:** `0x50` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Has Id | [Bool](./data-types#bool) | If true, a UUID follows. |
| Pack Id | [UUID](./data-types#uuid) | Optional. If present, removes that specific pack; otherwise pops all server-applied packs. |

**Semantics.** Unloads previously-pushed server resource packs.

## 0x51 - Resource Pack Push

**Packet ID:** `0x51` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Id | [UUID](./data-types#uuid) | Identifies the pack so it can be popped later. |
| Url | [String](./data-types#string) | Pack download URL. |
| Hash | [String](./data-types#string) | SHA-1 hash (40 chars) of the pack archive. |
| Required | [Bool](./data-types#bool) | If true, the client disconnects if the player declines. |
| Has Prompt | [Bool](./data-types#bool) | If true, a prompt component follows. |
| Prompt | [Text Component](./text-component) | Optional. Custom prompt message. |

**Semantics.** Asks the client to download and apply a server resource pack.

## 0x52 - Respawn

**Packet ID:** `0x52` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Common Spawn Info | Spawn Info | Same struct as in [Login](#0x31---login). |
| Data To Keep | [Byte](./data-types#byte) | Bitset: `0x01` keep attribute modifiers, `0x02` keep entity data, `0x03` keep all. |

**Semantics.** Respawns the player into a (possibly different) dimension. Equivalent to sending `Login` again but reusing the existing entity.

## 0x53 - Rotate Head

**Packet ID:** `0x53` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Entity Id | [VarInt](./data-types#varint) | Target. |
| Yaw | [Angle](./data-types#angle) | New head yaw (packed degrees). |

**Semantics.** Updates an entity's head rotation independently of its body yaw.

## 0x54 - Section Blocks Update

**Packet ID:** `0x54` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Section Position | [Long](./data-types#long) | Packed `SectionPos` — chunk section X/Y/Z. |
| Records | Array of [Long](./data-types#long) | Each `Long` packs `(BlockState << 12) | (relPos within section)` where `relPos` is `(x<<8)|(z<<4)|y`. Length is sent as a VarInt. |

**Semantics.** Batches multiple block changes within one chunk section.

## 0x55 - Select Advancements Tab

**Packet ID:** `0x55` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Has Tab | [Bool](./data-types#bool) | If true, a tab id follows. |
| Tab | [Identifier](./data-types#identifier) | Optional. Advancement tab to switch to. |

**Semantics.** Forces the advancements GUI to display a particular tab (or none).

## 0x56 - Server Data

**Packet ID:** `0x56` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| MOTD | [Text Component](./text-component) | Server description. |
| Has Icon | [Bool](./data-types#bool) | If true, raw icon bytes follow. |
| Icon Bytes | [Byte Array](./data-types#byte-array) | Optional. PNG bytes of the favicon. |

**Semantics.** Sends the server description / icon to the client (after the connection is established, distinct from status response).

## 0x57 - Set Action Bar Text

**Packet ID:** `0x57` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Text | [Text Component](./text-component) | Action-bar message. |

**Semantics.** Displays a temporary message above the hotbar.

## 0x58 - Set Border Center

**Packet ID:** `0x58` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Center X | [Double](./data-types#double) | |
| Center Z | [Double](./data-types#double) | |

**Semantics.** Updates the world border centre.

## 0x59 - Set Border Lerp Size

**Packet ID:** `0x59` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Old Diameter | [Double](./data-types#double) | Current diameter. |
| New Diameter | [Double](./data-types#double) | Target diameter. |
| Lerp Time | [VarLong](./data-types#varlong) | Milliseconds over which to interpolate. |

**Semantics.** Smoothly resizes the world border.

## 0x5A - Set Border Size

**Packet ID:** `0x5A` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Diameter | [Double](./data-types#double) | New border diameter, applied immediately. |

**Semantics.** Snap-resizes the world border.

## 0x5B - Set Border Warning Delay

**Packet ID:** `0x5B` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Warning Time | [VarInt](./data-types#varint) | Seconds until shrink at which the warning triggers. |

**Semantics.** Configures the time-based border warning threshold.

## 0x5C - Set Border Warning Distance

**Packet ID:** `0x5C` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Warning Blocks | [VarInt](./data-types#varint) | Distance from the border (in blocks) for the warning shader. |

**Semantics.** Configures the distance-based border warning threshold.

## 0x5D - Set Camera

**Packet ID:** `0x5D` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Camera Entity Id | [VarInt](./data-types#varint) | Entity whose POV the player views; the player's own id resets to first-person. |

**Semantics.** Switches the player's view to another entity (used by spectator mode and `/spectate`).

## 0x5E - Set Chunk Cache Center

**Packet ID:** `0x5E` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Chunk X | [VarInt](./data-types#varint) | |
| Chunk Z | [VarInt](./data-types#varint) | |

**Semantics.** Tells the client the new centre chunk for its loaded-chunk square (used to evict stale chunks).

## 0x5F - Set Chunk Cache Radius

**Packet ID:** `0x5F` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| View Distance | [VarInt](./data-types#varint) | Server-imposed view distance in chunks. |

**Semantics.** Updates the client's effective view distance.

## 0x60 - Set Cursor Item

**Packet ID:** `0x60` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Contents | [Slot](./slot) | New stack on the cursor. |

**Semantics.** Updates the item the player is dragging on the cursor independently of any container.

## 0x61 - Set Default Spawn Position

**Packet ID:** `0x61` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Respawn Data | RespawnData | `(Position BlockPos, Angle Float)` indicating world spawn / compass target. |

**Semantics.** Updates the world's default spawn point (also sets the compass target).

## 0x62 - Set Display Objective

**Packet ID:** `0x62` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Slot | [VarInt](./data-types#varint) | Display slot id (`0` list, `1` sidebar, `2` below name, `3`–`18` team-coloured sidebars). |
| Objective Name | [String](./data-types#string) | Empty string clears the slot. |

**Semantics.** Selects which scoreboard objective is rendered in a particular display slot.

## 0x63 - Set Entity Data

**Packet ID:** `0x63` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Entity Id | [VarInt](./data-types#varint) | Target. |
| Metadata | Entity Metadata | Sequence of `(Index Byte, Type VarInt, Value)` triplets terminated by index `0xFF`. See [entity-metadata](./entity-metadata). |

**Semantics.** Updates one or more synced data values on an entity (pose, health, custom name, etc.).

## 0x64 - Set Entity Link

**Packet ID:** `0x64` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Source Entity Id | [Int](./data-types#int) | Leashed entity (32-bit int). |
| Destination Entity Id | [Int](./data-types#int) | Holder; `0` clears the leash. |

**Semantics.** Sets / clears the leash link between two entities.

## 0x65 - Set Entity Motion

**Packet ID:** `0x65` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Entity Id | [VarInt](./data-types#varint) | Target. |
| Velocity X / Y / Z | [Short](./data-types#short) × 3 | Encoded as for [Add Entity](#0x01---add-entity). |

**Semantics.** Sets the entity velocity directly.

## 0x66 - Set Equipment

**Packet ID:** `0x66` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Entity Id | [VarInt](./data-types#varint) | Target. |
| Slots | Variable Array | Each entry is `(Slot Byte, Item Slot)`; the high bit (`0x80`) of `Slot` indicates more entries follow. Slot lo-bits encode the equipment slot ordinal (main hand, off hand, feet, legs, chest, head, body, saddle). |

**Semantics.** Updates one or more equipment slots on an entity in a single packet.

## 0x67 - Set Experience

**Packet ID:** `0x67` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Experience Bar | [Float](./data-types#float) | `0.0`–`1.0` filled fraction. |
| Experience Level | [VarInt](./data-types#varint) | |
| Total Experience | [VarInt](./data-types#varint) | Lifetime XP, used for the death drop calculation. |

**Semantics.** Updates the experience HUD.

## 0x68 - Set Health

**Packet ID:** `0x68` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Health | [Float](./data-types#float) | `0.0`–max-health hit points. |
| Food | [VarInt](./data-types#varint) | `0`–`20`. |
| Saturation | [Float](./data-types#float) | `0.0`–`5.0`. |

**Semantics.** Updates the health/food/saturation HUD.

## 0x69 - Set Held Slot

**Packet ID:** `0x69` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Slot | [VarInt](./data-types#varint) | `0`–`8` hotbar index. |

**Semantics.** Forces the active hotbar slot on the client.

## 0x6A - Set Objective

**Packet ID:** `0x6A` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Objective Name | [String](./data-types#string) | Identifier. |
| Method | [Byte](./data-types#byte) | `0` add, `1` remove, `2` change. |
| Display Name | [Text Component](./text-component) | Present for add/change. |
| Render Type | [Enum](./data-types#enum) ([VarInt](./data-types#varint)) | `0` INTEGER, `1` HEARTS. Present for add/change. |
| Has Number Format | [Bool](./data-types#bool) | Present for add/change. |
| Number Format | NumberFormat | Optional. Tagged variant: BLANK / STYLED (Style) / FIXED (Text Component). |

**Semantics.** Creates, removes or modifies a scoreboard objective.

## 0x6B - Set Passengers

**Packet ID:** `0x6B` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Vehicle Entity Id | [VarInt](./data-types#varint) | |
| Passenger Entity Ids | [VarInt](./data-types#varint) Array | All passengers, in seating order. |

**Semantics.** Sets the full passenger list of a vehicle.

## 0x6C - Set Player Inventory

**Packet ID:** `0x6C` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Slot | [VarInt](./data-types#varint) | Player inventory slot index. |
| Contents | [Slot](./slot) | New stack. |

**Semantics.** Slim alternative to [Container Set Slot](#0x14---container-set-slot) for the player's own inventory; does not require an open container.

## 0x6D - Set Player Team

**Packet ID:** `0x6D` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Team Name | [String](./data-types#string) | Identifier. |
| Method | [Byte](./data-types#byte) | `0` create, `1` remove, `2` update info, `3` add players, `4` remove players. |
| Parameters | TeamParameters | Present for create / update info. `(Display Name TC, Friendly Flags Byte, Visibility String[40], Collision Rule String[40], Team Color Enum, Prefix TC, Suffix TC)`. |
| Players | Array of [String](./data-types#string) | Present for create / add / remove. List of player names or fake-player ids. |

**Semantics.** Manages scoreboard teams (membership and display).

## 0x6E - Set Score

**Packet ID:** `0x6E` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Owner | [String](./data-types#string) | Player or fake-player. |
| Objective Name | [String](./data-types#string) | Objective being updated. |
| Value | [VarInt](./data-types#varint) | New score. |
| Has Display Name | [Bool](./data-types#bool) | Optional override. |
| Display Name | [Text Component](./text-component) | Optional. |
| Has Number Format | [Bool](./data-types#bool) | |
| Number Format | NumberFormat | Optional. |

**Semantics.** Sets a scoreboard score, optionally with a per-entry custom display.

## 0x6F - Set Simulation Distance

**Packet ID:** `0x6F` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Distance | [VarInt](./data-types#varint) | Simulation radius in chunks. |

**Semantics.** Updates the simulation-distance value previously sent in [Login](#0x31---login).

## 0x70 - Set Subtitle Text

**Packet ID:** `0x70` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Text | [Text Component](./text-component) | Subtitle. |

**Semantics.** Sets the subtitle of the next title; queue with [Set Title Text](#0x72---set-title-text) and [Set Titles Animation](#0x73---set-titles-animation).

## 0x71 - Set Time

**Packet ID:** `0x71` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Game Time | [Long](./data-types#long) | Total ticks the world has run. |
| Clock Updates | Map | `Map<Holder<WorldClock>, ClockNetworkState>`: per-clock state (e.g. day clock with `(time-of-day Long, scale Float, frozen Bool)`). |

**Semantics.** Synchronises wall-clock and per-dimension clocks (day/night, the End clock, etc.).

## 0x72 - Set Title Text

**Packet ID:** `0x72` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Text | [Text Component](./text-component) | Title text. |

**Semantics.** Sets the main title; pair with [Set Subtitle Text](#0x70---set-subtitle-text) and [Set Titles Animation](#0x73---set-titles-animation), then trigger via the next title display.

## 0x73 - Set Titles Animation

**Packet ID:** `0x73` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Fade In | [Int](./data-types#int) | Ticks. |
| Stay | [Int](./data-types#int) | Ticks. |
| Fade Out | [Int](./data-types#int) | Ticks. |

**Semantics.** Sets the timings for upcoming title displays.

## 0x74 - Sound Entity

**Packet ID:** `0x74` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Sound | Sound Holder | `(Registry Id VarInt, Optional Override (Identifier + Optional Range Float))`. |
| Source | [Enum](./data-types#enum) ([VarInt](./data-types#varint)) | Sound category (`MASTER`, `MUSIC`, `RECORDS`, `WEATHER`, `BLOCKS`, `HOSTILE`, `NEUTRAL`, `PLAYERS`, `AMBIENT`, `VOICE`, `UI`). |
| Entity Id | [VarInt](./data-types#varint) | Source entity. |
| Volume | [Float](./data-types#float) | Multiplier. |
| Pitch | [Float](./data-types#float) | `0.5`–`2.0`. |
| Seed | [Long](./data-types#long) | Used to deterministically pick variants. |

**Semantics.** Plays a sound attached to an entity (so it tracks if the entity moves).

## 0x75 - Sound

**Packet ID:** `0x75` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Sound | Sound Holder | See [Sound Entity](#0x74---sound-entity). |
| Source | [Enum](./data-types#enum) ([VarInt](./data-types#varint)) | Sound category. |
| Position X | [Int](./data-types#int) | `floor(x * 8)`. |
| Position Y | [Int](./data-types#int) | `floor(y * 8)`. |
| Position Z | [Int](./data-types#int) | `floor(z * 8)`. |
| Volume | [Float](./data-types#float) | |
| Pitch | [Float](./data-types#float) | |
| Seed | [Long](./data-types#long) | |

**Semantics.** Plays a sound at a fixed world position with eighth-block accuracy.

## 0x76 - Start Configuration

**Packet ID:** `0x76` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| (none) | — | Marker packet. |

**Semantics.** Terminal packet that asks the client to leave Play and re-enter the Configuration state. The client must reply with `configuration_acknowledged`.

## 0x77 - Stop Sound

**Packet ID:** `0x77` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Flags | [Byte](./data-types#byte) | `0x01` source present, `0x02` sound id present. |
| Source | [Enum](./data-types#enum) ([VarInt](./data-types#varint)) | Optional. Source category to silence. |
| Sound Id | [Identifier](./data-types#identifier) | Optional. Specific sound to stop. |

**Semantics.** Stops sounds matching the given filter; with no flags set, stops all sounds.

## 0x78 - Store Cookie

**Packet ID:** `0x78` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Key | [Identifier](./data-types#identifier) | Cookie key. |
| Payload | [Byte Array](./data-types#byte-array) | Up to 5120 bytes; persisted by the client across sessions/transfers. |

**Semantics.** Asks the client to persist a small key/value pair for later retrieval via [Cookie Request](#0x15---cookie-request).

## 0x79 - System Chat

**Packet ID:** `0x79` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Content | [Text Component](./text-component) | Message body. |
| Overlay | [Bool](./data-types#bool) | If true, render in the action-bar slot instead of the chat window. |

**Semantics.** Server-side, unsigned chat message (no sender identity).

## 0x7A - Tab List

**Packet ID:** `0x7A` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Header | [Text Component](./text-component) | Top of the player list. |
| Footer | [Text Component](./text-component) | Bottom of the player list. |

**Semantics.** Sets the per-player header/footer of the tab list.

## 0x7B - Tag Query

**Packet ID:** `0x7B` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Transaction Id | [VarInt](./data-types#varint) | Echoes the request's transaction id. |
| Tag | [NBT Compound](./data-types#nbt) (nullable) | Resulting NBT, or null if none. |

**Semantics.** Reply to a `data get`-style block-entity / entity NBT query.

## 0x7C - Take Item Entity

**Packet ID:** `0x7C` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Item Entity Id | [VarInt](./data-types#varint) | Item being picked up. |
| Player Entity Id | [VarInt](./data-types#varint) | Picker. |
| Amount | [VarInt](./data-types#varint) | Number of items collected. |

**Semantics.** Plays the pickup animation/sound (item flies into the player). The actual entity removal still arrives via [Remove Entities](#0x4d---remove-entities).

## 0x7D - Teleport Entity

**Packet ID:** `0x7D` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Entity Id | [VarInt](./data-types#varint) | Target. |
| Change | PositionMoveRotation | `(Position Vec3, Delta Vec3, Yaw Float, Pitch Float)`. |
| Relatives | Bitset of `Relative` | See [Player Position](#0x48---player-position). |
| On Ground | [Bool](./data-types#bool) | |

**Semantics.** Authoritative absolute teleport for any entity.

## 0x7E - Test Instance Block Status

**Packet ID:** `0x7E` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Status | [Text Component](./text-component) | Human-readable status. |
| Has Size | [Bool](./data-types#bool) | If true, three VarInts (Vec3i) follow. |
| Size | Vec3i | Optional. Size of the test instance bounding box. |

**Semantics.** Reports the status of the gametest instance block to the editing player.

## 0x7F - Ticking State

**Packet ID:** `0x7F` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Tick Rate | [Float](./data-types#float) | Server ticks per second (default 20). |
| Is Frozen | [Bool](./data-types#bool) | If true, world ticks are paused. |

**Semantics.** Updates the client's view of the server tick clock (`/tick rate`, `/tick freeze`).

## 0x80 - Ticking Step

**Packet ID:** `0x80` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Tick Steps | [VarInt](./data-types#varint) | Number of steps to advance while frozen. |

**Semantics.** Advances the frozen world by N ticks (`/tick step`).

## 0x81 - Transfer

**Packet ID:** `0x81` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Host | [String](./data-types#string) | Target server hostname. |
| Port | [VarInt](./data-types#varint) | Target server port. |

**Semantics.** Asks the client to seamlessly reconnect to another server while preserving stored cookies.

## 0x82 - Update Advancements

**Packet ID:** `0x82` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Reset | [Bool](./data-types#bool) | If true, the client clears its advancement set first. |
| Added | Array of AdvancementHolder | New / updated advancements (id, parent?, display?, requirements, sends_telemetry_event). |
| Removed | Array of [Identifier](./data-types#identifier) | Advancement ids to delete. |
| Progress | Map of ([Identifier](./data-types#identifier), AdvancementProgress) | Per-criterion completion timestamps. |
| Show Advancements | [Bool](./data-types#bool) | Whether to show the advancement screen. |

**Semantics.** Synchronises the client advancements view.

## 0x83 - Update Attributes

**Packet ID:** `0x83` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Entity Id | [VarInt](./data-types#varint) | Target. |
| Attributes | Array of AttributeSnapshot | Each `(Attribute Holder, Base Value Double, Modifiers Array)` where each modifier is `(Identifier, Amount Double, Operation Enum (ADD / MULTIPLY_BASE / MULTIPLY_TOTAL))`. |

**Semantics.** Updates the entity's attribute base values and active modifiers (max health, movement speed, etc.).

## 0x84 - Update Mob Effect

**Packet ID:** `0x84` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Entity Id | [VarInt](./data-types#varint) | Target. |
| Effect | [VarInt](./data-types#varint) | Registry id into `minecraft:mob_effect`. |
| Amplifier | [VarInt](./data-types#varint) | Effect level minus one (`0` = level I). |
| Duration | [VarInt](./data-types#varint) | Ticks remaining; `-1` = infinite. |
| Flags | [Byte](./data-types#byte) | `0x01` ambient, `0x02` show particles, `0x04` show icon, `0x08` blend. |

**Semantics.** Applies or refreshes a mob effect on an entity.

## 0x85 - Update Recipes

**Packet ID:** `0x85` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Item Sets | Map of (Property Set Key, Property Set) | Per-property recipe input sets (e.g. furnace fuels, smithing templates). |
| Stonecutter Recipes | SelectableRecipe.SingleInputSet | Special list of (input Ingredient, display) pairs for the stonecutter UI. |

**Semantics.** Sends the recipe property data needed for the recipe-driven UIs. Recipe display data is sent separately via [Recipe Book Add](#0x4a---recipe-book-add).

## 0x86 - Update Tags

**Packet ID:** `0x86` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Tags | Map | `Map<Registry ResourceKey, NetworkPayload>` where each NetworkPayload is `Map<Tag Identifier, Array of VarInt registry ids>`. See [tags](./tags). |

**Semantics.** Synchronises every tag in every registry with the client.

## 0x87 - Projectile Power

**Packet ID:** `0x87` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Entity Id | [VarInt](./data-types#varint) | Projectile (typically a wind charge). |
| Acceleration Power | [Double](./data-types#double) | Server-driven acceleration magnitude. |

**Semantics.** Adjusts the per-tick acceleration of a server-managed projectile.

## 0x88 - Custom Report Details

**Packet ID:** `0x88` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Details | Map of ([String](./data-types#string), [String](./data-types#string)) | Up to 32 entries; keys ≤ 128 chars, values ≤ 4096 chars. |

**Semantics.** Extra fields the client should include in any local crash report it generates while connected.

## 0x89 - Server Links

**Packet ID:** `0x89` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Links | Array of UntrustedEntry | Each `(Label Variant, Url String)`. The label variant is either a known type id (Bug Report, Community Guidelines, Support, Status, Feedback, Community, Website, Forums, News, Announcements) or a custom Text Component. |

**Semantics.** Populates the server-supplied links shown in the pause menu and the disconnection screen.

## 0x8A - Waypoint

**Packet ID:** `0x8A` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Operation | [Enum](./data-types#enum) ([VarInt](./data-types#varint)) | `0` TRACK, `1` UNTRACK, `2` UPDATE. |
| Waypoint | TrackedWaypoint | Tagged variant: empty (untrack), `(Position Vec3i, Icon)`, `(Chunk ChunkPos, Icon)`, or `(Azimuth Float, Icon)`. Each variant carries a UUID identifier and the icon (style id + optional color int). |

**Semantics.** Adds, updates or removes a tracked waypoint marker on the player's HUD.

## 0x8B - Clear Dialog

**Packet ID:** `0x8B` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| (none) | — | Marker packet. |

**Semantics.** Closes any server-shown dialog window currently open on the client.

## 0x8C - Show Dialog

**Packet ID:** `0x8C` · **State:** Play · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Dialog | Holder of Dialog | Either a registry reference (id `0` + VarInt registry index minus one) or an inline definition (id `0` then `0`-byte then full Dialog NBT/codec data). The inline definition encodes the dialog type and per-type body (notice, confirmation, multi-action, server-links, dialog-list). |

**Semantics.** Opens a dialog screen on the client; user input flows back via the corresponding `custom_click_action` serverbound packet.

> Source: net/minecraft/network/protocol/game/GameProtocols.java and the corresponding Clientbound*Packet.java files in the same directory.
