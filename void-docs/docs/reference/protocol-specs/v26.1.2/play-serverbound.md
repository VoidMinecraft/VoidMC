# Play — Serverbound Packets

This page is the authoritative serverbound specification for the Play state of Minecraft Java Edition protocol 26.1.2 (1.21.10). It enumerates every packet the client may send while in the Play protocol — both the packets defined in the dedicated `game` namespace and the shared `common` / `cookie` / `ping` packets that are also valid in Play. Packet identifiers are assigned by registration index in the Play protocol builder, sorted in the order shown below.

Field types referenced here (VarInt, VarLong, Identifier, BlockPos, Vec3, Slot, etc.) are defined in [./data-types](./data-types).

## Packet Index

| ID | Name |
|----|------|
| 0x00 | [Confirm Teleportation](#confirm-teleportation) |
| 0x01 | [Attack](#attack) |
| 0x02 | [Query Block Entity Tag](#query-block-entity-tag) |
| 0x03 | [Bundle Item Selected](#bundle-item-selected) |
| 0x04 | [Change Difficulty](#change-difficulty) |
| 0x05 | [Change Game Mode](#change-game-mode) |
| 0x06 | [Acknowledge Message (Chat Ack)](#acknowledge-message) |
| 0x07 | [Chat Command (unsigned)](#chat-command) |
| 0x08 | [Signed Chat Command](#signed-chat-command) |
| 0x09 | [Chat Message](#chat-message) |
| 0x0A | [Player Session (Chat Session Update)](#player-session) |
| 0x0B | [Chunk Batch Received](#chunk-batch-received) |
| 0x0C | [Client Status (Client Command)](#client-status) |
| 0x0D | [Client Tick End](#client-tick-end) |
| 0x0E | [Client Information](#client-information) |
| 0x0F | [Command Suggestions Request](#command-suggestions-request) |
| 0x10 | [Acknowledge Configuration](#acknowledge-configuration) |
| 0x11 | [Click Container Button](#click-container-button) |
| 0x12 | [Click Container](#click-container) |
| 0x13 | [Close Container](#close-container) |
| 0x14 | [Change Container Slot State](#change-container-slot-state) |
| 0x15 | [Cookie Response](#cookie-response) |
| 0x16 | [Serverbound Plugin Message (Custom Payload)](#serverbound-plugin-message) |
| 0x17 | [Debug Sample Subscription Request](#debug-sample-subscription-request) |
| 0x18 | [Edit Book](#edit-book) |
| 0x19 | [Query Entity Tag](#query-entity-tag) |
| 0x1A | [Interact](#interact) |
| 0x1B | [Jigsaw Generate](#jigsaw-generate) |
| 0x1C | [Serverbound Keep Alive](#serverbound-keep-alive) |
| 0x1D | [Lock Difficulty](#lock-difficulty) |
| 0x1E | [Set Player Position](#set-player-position) |
| 0x1F | [Set Player Position and Rotation](#set-player-position-and-rotation) |
| 0x20 | [Set Player Rotation](#set-player-rotation) |
| 0x21 | [Set Player Movement Flags](#set-player-movement-flags) |
| 0x22 | [Move Vehicle](#move-vehicle) |
| 0x23 | [Paddle Boat](#paddle-boat) |
| 0x24 | [Pick Item from Block](#pick-item-from-block) |
| 0x25 | [Pick Item from Entity](#pick-item-from-entity) |
| 0x26 | [Ping Request](#ping-request) |
| 0x27 | [Place Recipe](#place-recipe) |
| 0x28 | [Player Abilities](#player-abilities) |
| 0x29 | [Player Action](#player-action) |
| 0x2A | [Player Command](#player-command) |
| 0x2B | [Player Input](#player-input) |
| 0x2C | [Player Loaded](#player-loaded) |
| 0x2D | [Pong](#pong) |
| 0x2E | [Change Recipe Book Settings](#change-recipe-book-settings) |
| 0x2F | [Set Seen Recipe](#set-seen-recipe) |
| 0x30 | [Rename Item](#rename-item) |
| 0x31 | [Resource Pack Response](#resource-pack-response) |
| 0x32 | [Seen Advancements](#seen-advancements) |
| 0x33 | [Select Trade](#select-trade) |
| 0x34 | [Set Beacon Effect](#set-beacon-effect) |
| 0x35 | [Set Held Item](#set-held-item) |
| 0x36 | [Program Command Block](#program-command-block) |
| 0x37 | [Program Command Block Minecart](#program-command-block-minecart) |
| 0x38 | [Set Creative Mode Slot](#set-creative-mode-slot) |
| 0x39 | [Set Game Rule](#set-game-rule) |
| 0x3A | [Program Jigsaw Block](#program-jigsaw-block) |
| 0x3B | [Program Structure Block](#program-structure-block) |
| 0x3C | [Set Test Block](#set-test-block) |
| 0x3D | [Update Sign](#update-sign) |
| 0x3E | [Spectate Entity](#spectate-entity) |
| 0x3F | [Swing Arm](#swing-arm) |
| 0x40 | [Teleport to Entity](#teleport-to-entity) |
| 0x41 | [Test Instance Block Action](#test-instance-block-action) |
| 0x42 | [Use Item On](#use-item-on) |
| 0x43 | [Use Item](#use-item) |
| 0x44 | [Custom Click Action](#custom-click-action) |

## Confirm Teleportation

**Packet ID:** `0x00` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Teleport ID | [VarInt](./data-types) | Echo of the `Teleport ID` from the most recent clientbound `Player Position` (Synchronize Player Position) packet. |

**Semantics.** Sent by the client immediately after applying a clientbound Player Position. Until the server receives an acknowledgement matching the latest pending teleport ID, all subsequent movement packets are ignored.

## Attack

**Packet ID:** `0x01` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types) | Network ID of the entity being attacked. |

**Semantics.** Compact convenience form of the more general Interact packet specifically for left-click attacks. Sent in addition to a `Swing Arm` packet.

> NOTE: This packet is a recent split from the legacy combined Interact packet; the older form (with attack/use/interact-at variants) has been replaced by this packet plus [Interact](#interact).

## Query Block Entity Tag

**Packet ID:** `0x02` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Transaction ID | [VarInt](./data-types) | Client-chosen identifier the server echoes in the `Tag Query Response` reply. |
| Location | [BlockPos](./data-types) | Position of the block entity to query. |

**Semantics.** Issued by debug clients (F3+I) to request the full NBT of a block entity. Requires operator permission level on the server.

## Bundle Item Selected

**Packet ID:** `0x03` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Slot of Bundle | [VarInt](./data-types) | Container slot containing the bundle whose selection is being changed. |
| Selected Item Index | [VarInt](./data-types) | Index of the item selected inside the bundle, or `-1` to clear the selection. Negative values other than `-1` are rejected. |

**Semantics.** Sent when the player scrolls to choose which item is currently surfaced by a bundle.

## Change Difficulty

**Packet ID:** `0x04` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| New Difficulty | [Byte](./data-types#byte) (enum) | `0`=Peaceful, `1`=Easy, `2`=Normal, `3`=Hard. |

**Semantics.** Single-player only. Servers ignore the packet unless the sender is OP and the difficulty is unlocked.

## Change Game Mode

**Packet ID:** `0x05` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Game Mode | [VarInt](./data-types#varint) (enum) | `0`=Survival, `1`=Creative, `2`=Adventure, `3`=Spectator. |

**Semantics.** Single-player only; sent by the host when toggling game mode via the F3+F4 menu. Vanilla servers reject this from regular clients.

## Acknowledge Message

**Packet ID:** `0x06` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Message Count | [VarInt](./data-types) | Number of entries to advance the client's "last seen" message tracker by. |

**Semantics.** Periodic acknowledgement that the client has consumed signed chat messages, advancing the windowed message-validation counter used by the secure chat system.

## Chat Command

**Packet ID:** `0x07` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Command | [String](./data-types#string) (≤256) | The command text without the leading `/`. |

**Semantics.** Sent when the client executes a slash command that has no signable argument components. Command-argument signatures, if any, are sent via [Signed Chat Command](#signed-chat-command).

## Signed Chat Command

**Packet ID:** `0x08` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Command | [String](./data-types#string) (≤256) | Command text without the leading `/`. |
| Timestamp | [Long](./data-types#long) | Client-side instant the command was issued, in epoch milliseconds. |
| Salt | [Long](./data-types#long) | Random salt used in the signature digest. |
| Argument Signatures | Array of ([String](./data-types#string) name, 256-byte Signature) | One entry per signed command argument. See [./chat](./chat). |
| Message Count | [VarInt](./data-types) | Total messages observed by the client at send time. |
| Acknowledged | [BitSet](./data-types#bitset) (20 bits, 3 bytes) | Bitset over the most recent 20 entries of the client's last-seen window. |

**Semantics.** Sent for commands that contain at least one argument requiring a chat signature (e.g. `/msg`, `/me`).

## Chat Message

**Packet ID:** `0x09` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Message | [String](./data-types#string) (≤256) | The chat message text. |
| Timestamp | [Long](./data-types#long) | Epoch milliseconds at which the client issued the message. |
| Salt | [Long](./data-types#long) | Random salt used in the signature digest. |
| Signature | Optional 256-byte Signature | Present only if the player has a chat session and signed the message. |
| Message Count | [VarInt](./data-types) | Total messages observed by the client at send time. |
| Acknowledged | [BitSet](./data-types#bitset) (20 bits, 3 bytes) | Window of recently seen messages used by the secure-chat algorithm. |

**Semantics.** Player-typed chat. The signature, when present, is verified against the player's chat session public key.

## Player Session

**Packet ID:** `0x0A` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Session ID | [UUID](./data-types#uuid) | Identifier of the new chat session. |
| Public Key Expires At | [Long](./data-types#long) | Mojang-issued expiry time of the public key, in epoch milliseconds. |
| Public Key | [Byte Array](./data-types#byte-array) (≤512) | DER-encoded RSA public key. |
| Key Signature | [Byte Array](./data-types#byte-array) (≤4096) | Mojang's signature over the public-key blob, proving the key was issued for this player. |

**Semantics.** Establishes or renews the per-player signing session used by secure chat. Sent right after login and whenever the chat key rotates.

## Chunk Batch Received

**Packet ID:** `0x0B` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Desired Chunks Per Tick | [Float](./data-types#float) | Throttle hint computed by the client based on observed batch latency. |

**Semantics.** Acknowledges the most recent `Chunk Batch Finished` and informs the server of the client's measured chunk-streaming throughput so the server can adapt batch size.

## Client Status

**Packet ID:** `0x0C` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Action ID | [VarInt](./data-types#varint) (enum) | `0`=Perform Respawn, `1`=Request Stats, `2`=Request Game Rule Values. |

**Semantics.** Multi-purpose client lifecycle command. `Perform Respawn` is sent when the player clicks the death-screen respawn button.

## Client Tick End

**Packet ID:** `0x0D` · **State:** Play · **Bound To:** Server

(empty)

**Semantics.** Marks the end of a client game tick. Sent every tick to let the server align tick-bound work (e.g. movement coalescing). Carries no fields.

## Client Information

**Packet ID:** `0x0E` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Locale | [String](./data-types#string) (≤16) | Client locale, e.g. `en_us`. |
| View Distance | [Byte](./data-types#byte) | Client-side render distance in chunks. |
| Chat Mode | [VarInt](./data-types#varint) (enum) | `0`=Enabled, `1`=Commands Only, `2`=Hidden. |
| Chat Colors | [Boolean](./data-types#boolean) | Whether chat colours are honoured by the client. |
| Displayed Skin Parts | [Unsigned Byte](./data-types#unsigned-byte) | Bitmask of cape/jacket/sleeve/pants/hat layers to render. |
| Main Hand | [VarInt](./data-types#varint) (enum) | `0`=Left, `1`=Right. |
| Enable Text Filtering | [Boolean](./data-types#boolean) | If true, the client opted in to server-side profanity filtering. |
| Allow Server Listings | [Boolean](./data-types#boolean) | Whether the player consents to appearing in the public player list. |
| Particle Status | [VarInt](./data-types#varint) (enum) | `0`=All, `1`=Decreased, `2`=Minimal. |

**Semantics.** Sent immediately after entering Play and re-sent whenever the client's settings change. Defined in the shared `common` namespace; identical to the configuration-state packet of the same name.

## Command Suggestions Request

**Packet ID:** `0x0F` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Transaction ID | [VarInt](./data-types) | Echoed back in the matching `Command Suggestions Response`. |
| Text | [String](./data-types#string) (≤2048) | Current contents of the chat input, including the leading `/`. |

**Semantics.** Asks the server to brigadier-parse the partial command text and return tab-completion candidates.

## Acknowledge Configuration

**Packet ID:** `0x10` · **State:** Play · **Bound To:** Server

(empty)

**Semantics.** Sent by the client in response to a clientbound `Start Configuration` packet. After sending, the client transitions to the Configuration state. This packet is `terminal`: no further Play packets may follow it on the wire.

## Click Container Button

**Packet ID:** `0x11` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Container ID | [VarInt](./data-types) | Identifier of the open container window. |
| Button ID | [VarInt](./data-types) | Container-specific button index (e.g. enchantment slot 0/1/2, lectern page-turn buttons). |

**Semantics.** Used by containers that expose explicit buttons rather than slot interactions.

## Click Container

**Packet ID:** `0x12` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Container ID | [VarInt](./data-types) | Identifier of the open container. |
| State ID | [VarInt](./data-types) | Server-issued state counter from the most recent `Container Set Content/Slot`; used to detect desync. |
| Slot | [Short](./data-types#short) | Slot number that was clicked, or `-999` for outside the window. |
| Button | [Byte](./data-types#byte) | Mouse button or click-variant byte; semantics depend on Mode. |
| Mode | [VarInt](./data-types#varint) (enum) | Click mode: `0`=normal click, `1`=shift-click, `2`=number-key, `3`=middle-click, `4`=drop, `5`=drag, `6`=double-click. |
| Changed Slots | Map([Short](./data-types#short) → [HashedStack](./slot)) | Up to 128 entries. Client's prediction of post-click slot contents, encoded as compact hashes. |
| Carried Item | [HashedStack](./slot) | Client's prediction of the cursor stack after the click. |

**Semantics.** Container interaction. The server recomputes the click and, if the client's predicted hashes diverge, replies with corrective `Container Set Slot` / `Container Set Content` packets. See [./slot](./slot) for the hashed-slot encoding.

## Close Container

**Packet ID:** `0x13` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Container ID | [VarInt](./data-types) | Identifier of the container being closed. |

**Semantics.** Notifies the server that the player closed an inventory screen. Servers may also close the window from their side via the clientbound counterpart.

## Change Container Slot State

**Packet ID:** `0x14` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Slot ID | [VarInt](./data-types) | Slot whose enabled-state is toggling. |
| Container ID | [VarInt](./data-types) | Identifier of the open container. |
| State | [Boolean](./data-types#boolean) | Whether the slot is now enabled. |

**Semantics.** Used by the crafter (3×3 auto-crafter) UI to enable or disable individual ingredient slots.

## Cookie Response

**Packet ID:** `0x15` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Key | [Identifier](./data-types) | Identifier of the cookie being returned. |
| Payload | Optional [Byte Array](./data-types#byte-array) (≤5120) | Stored cookie contents, or absent if the client holds no cookie under that key. |

**Semantics.** Reply to a clientbound `Cookie Request`. Cookies are small opaque blobs persisted on the client across server transfers (`Transfer` packet).

## Serverbound Plugin Message

**Packet ID:** `0x16` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Channel | [Identifier](./data-types) | Channel namespace, e.g. `minecraft:brand`. |
| Data | [Byte Array](./data-types#byte-array) (≤32767) | Channel-specific payload, length-implied (consume to end of packet). |

**Semantics.** Generic transport for mod/plugin extension channels. The vanilla client sends `minecraft:brand` carrying its brand string immediately after entering Play.

## Debug Sample Subscription Request

**Packet ID:** `0x17` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Subscriptions | Array of [VarInt](./data-types#varint) (registry IDs) | Set of `minecraft:debug_subscription` registry entries the client wants to receive samples for. |

**Semantics.** Subscribes the client to one or more debug feeds (e.g. tick-time samples). Operator-only.

## Edit Book

**Packet ID:** `0x18` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Slot | [VarInt](./data-types) | Inventory slot containing the writable book. |
| Pages | Array of [String](./data-types#string) (≤100 entries, each ≤1024 chars) | New page contents. |
| Title | Optional [String](./data-types#string) (≤32) | Present only when signing the book. |

**Semantics.** Saves an in-progress writable book or signs it into a written book.

## Query Entity Tag

**Packet ID:** `0x19` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Transaction ID | [VarInt](./data-types) | Echoed in the `Tag Query Response`. |
| Entity ID | [VarInt](./data-types) | Network ID of the target entity. |

**Semantics.** Operator/debug request for the full NBT of an entity (F3+I on an entity).

## Interact

**Packet ID:** `0x1A` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types) | Target entity. |
| Hand | [VarInt](./data-types#varint) (enum) | `0`=Main Hand, `1`=Off Hand. |
| Target | [Vec3](./data-types) of three Floats | Local hit position relative to the entity, used by interact-at hitboxes. |
| Using Secondary Action | [Boolean](./data-types#boolean) | True if the player was sneaking when the interaction occurred. |

**Semantics.** Right-click / interact-at action against an entity. Pure left-click attacks use the dedicated [Attack](#attack) packet.

> NOTE: The single Java packet encodes interact-at semantics. The presence of three floats is unconditional; the previous "type" discriminator was removed when [Attack](#attack) was split out.

## Jigsaw Generate

**Packet ID:** `0x1B` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Location | [BlockPos](./data-types) | Position of the jigsaw block requesting generation. |
| Levels | [VarInt](./data-types) | Number of recursive jigsaw layers to generate. |
| Keep Jigsaws | [Boolean](./data-types#boolean) | If true, leftover jigsaw blocks are not removed after generation. |

**Semantics.** Sent by the jigsaw block UI to trigger structure generation. Operator-only.

## Serverbound Keep Alive

**Packet ID:** `0x1C` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Keep Alive ID | [Long](./data-types#long) | Echo of the value from the most recent clientbound `Keep Alive`. |

**Semantics.** Liveness response. The server disconnects clients that fail to respond within the configured keep-alive window.

## Lock Difficulty

**Packet ID:** `0x1D` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Locked | [Boolean](./data-types#boolean) | New locked state. |

**Semantics.** Single-player only; sent by the host to lock or unlock the world's difficulty.

## Set Player Position

**Packet ID:** `0x1E` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| X | [Double](./data-types#double) | New player X. |
| Feet Y | [Double](./data-types#double) | New player Y at the feet. |
| Z | [Double](./data-types#double) | New player Z. |
| Flags | [Unsigned Byte](./data-types#unsigned-byte) | Bit 0 = on ground, bit 1 = horizontal collision. |

**Semantics.** Sent each tick the player's position changed but rotation did not.

## Set Player Position and Rotation

**Packet ID:** `0x1F` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| X | [Double](./data-types#double) | New X. |
| Feet Y | [Double](./data-types#double) | New feet Y. |
| Z | [Double](./data-types#double) | New Z. |
| Yaw | [Float](./data-types#float) | Absolute yaw in degrees. |
| Pitch | [Float](./data-types#float) | Absolute pitch in degrees. |
| Flags | [Unsigned Byte](./data-types#unsigned-byte) | Bit 0 = on ground, bit 1 = horizontal collision. |

**Semantics.** Sent each tick when both position and rotation changed.

## Set Player Rotation

**Packet ID:** `0x20` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Yaw | [Float](./data-types#float) | Absolute yaw in degrees. |
| Pitch | [Float](./data-types#float) | Absolute pitch in degrees. |
| Flags | [Unsigned Byte](./data-types#unsigned-byte) | Bit 0 = on ground, bit 1 = horizontal collision. |

**Semantics.** Sent each tick when only rotation changed.

## Set Player Movement Flags

**Packet ID:** `0x21` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Flags | [Unsigned Byte](./data-types#unsigned-byte) | Bit 0 = on ground, bit 1 = horizontal collision. |

**Semantics.** Sent each tick when neither position nor rotation changed but the on-ground / horizontal-collision status must still be reported. Corresponds to the `Move Player Status Only` Java class.

## Move Vehicle

**Packet ID:** `0x22` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Position | [Vec3](./data-types) (3 Doubles) | New vehicle position. |
| Yaw | [Float](./data-types#float) | New vehicle yaw. |
| Pitch | [Float](./data-types#float) | New vehicle pitch. |
| On Ground | [Boolean](./data-types#boolean) | Vehicle's on-ground flag. |

**Semantics.** Sent while the player drives a controllable vehicle (boats, certain minecarts) to authoritatively report its position.

## Paddle Boat

**Packet ID:** `0x23` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Left Paddle Turning | [Boolean](./data-types#boolean) | Whether the left paddle is currently being rowed. |
| Right Paddle Turning | [Boolean](./data-types#boolean) | Whether the right paddle is currently being rowed. |

**Semantics.** Drives boat-paddle animation when the player controls a boat.

## Pick Item from Block

**Packet ID:** `0x24` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Position | [BlockPos](./data-types) | Block targeted by middle-click. |
| Include Data | [Boolean](./data-types#boolean) | If true, the resulting picked item should preserve the block entity's data components. |

**Semantics.** Sent when the player middle-clicks a block in creative mode to obtain its item form.

## Pick Item from Entity

**Packet ID:** `0x25` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types) | Network ID of the entity targeted by middle-click. |
| Include Data | [Boolean](./data-types#boolean) | If true, preserve the entity's data components on the picked item (e.g. spawn-egg variant). |

**Semantics.** Creative middle-click on an entity.

## Ping Request

**Packet ID:** `0x26` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Payload | [Long](./data-types#long) | Arbitrary value the server echoes back in `Pong Response`. |

**Semantics.** Out-of-band latency probe. Distinct from Keep Alive in that the client chooses both the timing and the payload.

## Place Recipe

**Packet ID:** `0x27` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Container ID | [VarInt](./data-types) | Identifier of the open crafting/recipe-book container. |
| Recipe | RecipeDisplayId ([VarInt](./data-types#varint)) | Recipe-display identifier from the recipe book. |
| Use Max Items | [Boolean](./data-types#boolean) | If true (shift-click), distribute items to fill the recipe to its maximum. |

**Semantics.** Sent when the player clicks a recipe in the recipe book to lay it out into the crafting grid.

## Player Abilities

**Packet ID:** `0x28` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Flags | [Byte](./data-types#byte) | Bit 1 (`0x02`) set = flying. All other bits are reserved/ignored on the serverbound side. |

**Semantics.** Reports a change to the player's flying state (toggling creative/spectator flight). The server uses this to keep its abilities snapshot in sync.

## Player Action

**Packet ID:** `0x29` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Status | [VarInt](./data-types#varint) (enum) | `0`=Start digging, `1`=Cancel digging, `2`=Finish digging, `3`=Drop full stack, `4`=Drop one item, `5`=Release use item (e.g. shoot bow), `6`=Swap held with off-hand, `7`=Stab. |
| Location | [BlockPos](./data-types) | Block being affected. Zero for non-block actions. |
| Face | [Unsigned Byte](./data-types#unsigned-byte) | Block face (`Direction.from3DDataValue`): `0`=Down, `1`=Up, `2`=North, `3`=South, `4`=West, `5`=East. |
| Sequence | [VarInt](./data-types) | Client prediction sequence id, echoed via `Block Changed Ack` to commit/rollback the predicted block change. |

**Semantics.** Catch-all for non-movement player actions tied to the world. The `STAB` action was added alongside the bundled `Attack` split.

## Player Command

**Packet ID:** `0x2A` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types) | Acting entity, normally the player itself. |
| Action ID | [VarInt](./data-types#varint) (enum) | `0`=Stop sleeping, `1`=Start sprinting, `2`=Stop sprinting, `3`=Start jump (horse), `4`=Stop jump (horse), `5`=Open horse/llama inventory, `6`=Start fall flying (elytra). |
| Jump Boost | [VarInt](./data-types) | Horse jump charge `0..100`; `0` for non-jump actions. |

**Semantics.** Player state-machine transitions that don't fit the movement packets.

## Player Input

**Packet ID:** `0x2B` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Inputs | [Unsigned Byte](./data-types#unsigned-byte) (bitfield) | Bit 0 = forward, 1 = back, 2 = left, 3 = right, 4 = jump, 5 = sneak, 6 = sprint. |

**Semantics.** Reports the raw movement input keys the player is holding. Used by vehicles and by the server-side "anti-cheat" movement validator.

## Player Loaded

**Packet ID:** `0x2C` · **State:** Play · **Bound To:** Server

(empty)

**Semantics.** Sent once after the player's surroundings have been fully received (initial chunk load complete or post-respawn). Marks the moment the server may resume sending world events that depend on the player being "ready".

## Pong

**Packet ID:** `0x2D` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| ID | [Int](./data-types#int) (32-bit, big-endian, **not** a [VarInt](./data-types#varint)) | Echo of the integer payload from the most recent clientbound `Ping`. |

**Semantics.** Reply to the common-state `Ping` packet. Distinct from [Ping Request](#ping-request) — that one is client-initiated, this one is server-initiated.

## Change Recipe Book Settings

**Packet ID:** `0x2E` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Book ID | [VarInt](./data-types#varint) (enum) | `0`=Crafting, `1`=Furnace, `2`=Blast Furnace, `3`=Smoker. |
| Book Open | [Boolean](./data-types#boolean) | Whether the recipe book pane is currently open. |
| Filter Active | [Boolean](./data-types#boolean) | Whether the "show only craftable" filter is enabled. |

**Semantics.** Persists the player's per-book recipe-book UI state to the server.

## Set Seen Recipe

**Packet ID:** `0x2F` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Recipe | RecipeDisplayId ([VarInt](./data-types#varint)) | Recipe the player has clicked on (clearing its "new" highlight). |

**Semantics.** Marks a recipe-book entry as no longer requiring the "new!" badge.

## Rename Item

**Packet ID:** `0x30` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Item Name | [String](./data-types#string) | New name typed into the anvil text field. |

**Semantics.** Sent live as the player edits the anvil's name field, so the server can update the output preview and XP cost.

## Resource Pack Response

**Packet ID:** `0x31` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| UUID | [UUID](./data-types#uuid) | Identifier of the pack the response refers to (matches the push). |
| Result | [VarInt](./data-types#varint) (enum) | `0`=Successfully Loaded, `1`=Declined, `2`=Failed Download, `3`=Accepted, `4`=Downloaded, `5`=Invalid URL, `6`=Failed Reload, `7`=Discarded. |

**Semantics.** Reports progress for a resource pack offered via clientbound `Resource Pack Push`. `ACCEPTED` and `DOWNLOADED` are intermediate; all other values are terminal for the given UUID.

## Seen Advancements

**Packet ID:** `0x32` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Action | [VarInt](./data-types#varint) (enum) | `0`=Opened tab, `1`=Closed screen. |
| Tab ID | Optional [Identifier](./data-types) | Present iff Action=0. Identifier of the advancement tab now in focus. |

**Semantics.** Reports advancement-screen UI state so the server can stream advancement progress only for the currently visible tab.

## Select Trade

**Packet ID:** `0x33` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Selected Slot | [VarInt](./data-types) | Index of the villager trade the player chose. |

**Semantics.** Sent when the player picks an offer from the merchant UI; the server then prefills the trade input slots.

## Set Beacon Effect

**Packet ID:** `0x34` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Primary Effect | Optional [VarInt](./data-types#varint) (registry ID into `minecraft:mob_effect`) | The chosen primary effect, or absent for "none". |
| Secondary Effect | Optional [VarInt](./data-types#varint) (registry ID into `minecraft:mob_effect`) | The chosen secondary effect, or absent for "none". |

**Semantics.** Sent when the player confirms a beacon's effect selection. Consumes the iron ingot (or equivalent payment) on the server side.

## Set Held Item

**Packet ID:** `0x35` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Slot | [Short](./data-types#short) | Hotbar slot index `0..8`. |

**Semantics.** Notifies the server of a hotbar selection change.

## Program Command Block

**Packet ID:** `0x36` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Position | [BlockPos](./data-types) | Command block being edited. |
| Command | [String](./data-types#string) | New command text. |
| Mode | [VarInt](./data-types#varint) (enum) | `0`=Sequence, `1`=Auto, `2`=Redstone. |
| Flags | [Byte](./data-types#byte) | Bit 0 = Track Output, bit 1 = Conditional, bit 2 = Automatic. |

**Semantics.** Saves changes from the command block UI. Operator + creative mode required.

## Program Command Block Minecart

**Packet ID:** `0x37` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types) | The command-block minecart being edited. |
| Command | [String](./data-types#string) | New command text. |
| Track Output | [Boolean](./data-types#boolean) | If true, the last output line is stored on the entity. |

**Semantics.** Saves changes from the command-block minecart UI.

## Set Creative Mode Slot

**Packet ID:** `0x38` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Slot | [Short](./data-types#short) | Inventory slot index. |
| Item | [Slot](./slot) | New item stack (optional, untrusted variant). |

**Semantics.** Creative-mode "place this stack into that slot" operation. The server rejects this packet entirely if the player is not in a game mode with infinite materials.

## Set Game Rule

**Packet ID:** `0x39` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Entries | Array of (Game-Rule Key, [String](./data-types#string) value) | Each entry: a `ResourceKey` into the `minecraft:game_rule` registry, plus the textual new value. |

**Semantics.** Single-player only. Sent from the in-game rule-tweak menu.

## Program Jigsaw Block

**Packet ID:** `0x3A` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Position | [BlockPos](./data-types) | Jigsaw block being edited. |
| Name | [Identifier](./data-types) | Jigsaw "name" tag. |
| Target | [Identifier](./data-types) | Target jigsaw name to align against. |
| Pool | [Identifier](./data-types) | Template pool to draw from. |
| Final State | [String](./data-types#string) | Block-state string written into the world after generation. |
| Joint Type | [String](./data-types#string) (enum: `aligned`/`rollable`) | Default `aligned` if unrecognised. |
| Selection Priority | [VarInt](./data-types) | Higher priorities are processed first. |
| Placement Priority | [VarInt](./data-types) | Tiebreaker between equal selection priorities. |

**Semantics.** Saves changes from the jigsaw block UI. Operator-only.

## Program Structure Block

**Packet ID:** `0x3B` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Position | [BlockPos](./data-types) | Structure block being edited. |
| Action | [VarInt](./data-types#varint) (enum) | `0`=Update Data, `1`=Save, `2`=Load, `3`=Detect Size. |
| Mode | [VarInt](./data-types#varint) (enum) | `0`=Save, `1`=Load, `2`=Corner, `3`=Data. |
| Name | [String](./data-types#string) | Structure name. |
| Offset X / Y / Z | [Byte](./data-types#byte) each | Each clamped to `[-48, 48]`. |
| Size X / Y / Z | [Byte](./data-types#byte) each | Each clamped to `[0, 48]`. |
| Mirror | [VarInt](./data-types#varint) (enum) | `0`=None, `1`=Left-Right, `2`=Front-Back. |
| Rotation | [VarInt](./data-types#varint) (enum) | `0`=None, `1`=90 CW, `2`=180, `3`=270 CW. |
| Metadata | [String](./data-types#string) (≤128) | Mode-dependent metadata. |
| Integrity | [Float](./data-types#float) | Clamped to `[0.0, 1.0]`. Probability that any given block is included. |
| Seed | [VarLong](./data-types) | RNG seed for the integrity sampling. |
| Flags | [Byte](./data-types#byte) | Bit 0 = Ignore Entities, bit 1 = Show Air, bit 2 = Show Bounding Box, bit 3 = Strict. |

**Semantics.** Saves changes from the structure block UI. Operator + creative mode required.

## Set Test Block

**Packet ID:** `0x3C` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Position | [BlockPos](./data-types) | Test block being edited. |
| Mode | [VarInt](./data-types#varint) (enum) | Test-block mode (`START`, `LOG`, `FAIL`, `ACCEPT`). |
| Message | [String](./data-types#string) | Free-form message displayed/logged by the test block. |

**Semantics.** Saves changes from the test-block UI used by the in-game test framework.

## Update Sign

**Packet ID:** `0x3D` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Location | [BlockPos](./data-types) | Sign being edited. |
| Is Front Text | [Boolean](./data-types#boolean) | True for the front face, false for the back face. |
| Line 1..4 | Four Strings (≤384 each) | New text for each of the four lines. |

**Semantics.** Sent when the player closes the sign edit screen.

## Spectate Entity

**Packet ID:** `0x3E` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Target | [UUID](./data-types#uuid) | Entity the spectator wants to teleport to / attach to. |

**Semantics.** Spectator-mode only. Server resolves the UUID into an entity in the player's current world; otherwise the packet is ignored.

## Swing Arm

**Packet ID:** `0x3F` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Hand | [VarInt](./data-types#varint) (enum) | `0`=Main Hand, `1`=Off Hand. |

**Semantics.** Plays the arm-swing animation. Sent on left-click and on most use-item actions; the server rebroadcasts via `Entity Animation` to other clients.

## Teleport to Entity

**Packet ID:** `0x40` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Target Player | [UUID](./data-types#uuid) | UUID of the entity to teleport to. |

**Semantics.** Spectator-mode shortcut to jump to another entity by UUID.

## Test Instance Block Action

**Packet ID:** `0x41` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Position | [BlockPos](./data-types) | Test-instance block being acted on. |
| Action | [VarInt](./data-types#varint) (enum, by id) | `0`=Init, `1`=Query, `2`=Set, `3`=Reset, `4`=Save, `5`=Export, `6`=Run. Out-of-range values fall back to Init. |
| Test | Optional ResourceKey into `minecraft:test_instance` | Currently selected test, or absent. |
| Size | [Vec3i](./data-types) (3 VarInts) | Bounding-box size. |
| Rotation | [VarInt](./data-types#varint) (enum) | `Rotation` enum: `NONE`, `CLOCKWISE_90`, `CLOCKWISE_180`, `COUNTERCLOCKWISE_90`. |
| Ignore Entities | [Boolean](./data-types#boolean) | If true, entities are excluded from save/run. |

**Semantics.** Saves changes to or executes a test-instance block (the more general successor to the legacy structure-test block).

## Use Item On

**Packet ID:** `0x42` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Hand | [VarInt](./data-types#varint) (enum) | `0`=Main Hand, `1`=Off Hand. |
| Block Hit Result | [BlockHitResult](./data-types#blockhitresult) | Encoded as: BlockPos (Long), Face (VarInt 0..5), three Floats giving the cursor position relative to the block (`0..1` per axis), Boolean "inside block", Boolean "world border hit". |
| Sequence | [VarInt](./data-types) | Client prediction sequence id; echoed via `Block Changed Ack`. |

**Semantics.** Right-click on a block. Most block placements, opens, and item-on-block uses go through this packet.

## Use Item

**Packet ID:** `0x43` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Hand | [VarInt](./data-types#varint) (enum) | `0`=Main Hand, `1`=Off Hand. |
| Sequence | [VarInt](./data-types) | Client prediction sequence id. |
| Yaw | [Float](./data-types#float) | Player yaw at the moment of use. |
| Pitch | [Float](./data-types#float) | Player pitch at the moment of use. |

**Semantics.** Right-click in the air or on a non-block target (e.g. drinking a potion, throwing a snowball). Yaw/pitch are sent so the server can use the same view direction the client did when computing projectile trajectories.

## Custom Click Action

**Packet ID:** `0x44` · **State:** Play · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| ID | [Identifier](./data-types) | Identifier of the custom click action declared by the server (e.g. via a dialog or system message). |
| Payload | Optional [NBT](./data-types#nbt) [Tag](./data-types) | Untrusted tag (≤32 KiB, depth ≤16, length-prefixed ≤64 KiB). |

**Semantics.** Sent in response to a `custom`-type click event embedded in chat components or in a `Show Dialog` packet. Lets the server dispatch UI-driven actions without round-tripping through commands.

> Source: net/minecraft/network/protocol/game/GameProtocols.java and the corresponding Serverbound*Packet.java files in the same directory.
