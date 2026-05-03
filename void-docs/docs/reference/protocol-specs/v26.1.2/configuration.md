# Configuration State

The Configuration state is entered after the client sends [Login Acknowledged](./login#login-acknowledged). Its purpose is to negotiate the data the client needs *before* spawning into a world: client settings, registry contents (dimension types, biomes, damage types, …), enabled feature flags, tag bindings, the resource pack, and various housekeeping items (cookies, server links, custom report details, the code-of-conduct, the dialog system). The state ends when the server sends [Finish Configuration](#finish-configuration-clientbound) and the client replies with [Finish Configuration](#finish-configuration-serverbound), at which point both sides switch to Play.

The Configuration state may be re-entered from Play (vanilla calls this "reconfiguration") to swap the registry set, e.g. when transferring the player to a new dimension stack.

See [Data Types](./data-types), [Registries](./registries), [Tags](./tags), and [Text Component](./text-component) for the encodings of compound fields.

| ID | Name | Direction |
|----|------|-----------|
| `0x00` | [Cookie Request](#cookie-request) | Client-bound |
| `0x01` | [Custom Payload (Plugin Message)](#custom-payload-plugin-message) | Client-bound |
| `0x02` | [Disconnect](#disconnect) | Client-bound |
| `0x03` | [Finish Configuration](#finish-configuration-clientbound) | Client-bound |
| `0x04` | [Keep Alive](#keep-alive) | Client-bound |
| `0x05` | [Ping](#ping) | Client-bound |
| `0x06` | [Reset Chat](#reset-chat) | Client-bound |
| `0x07` | [Registry Data](#registry-data) | Client-bound |
| `0x08` | [Resource Pack Pop](#resource-pack-pop) | Client-bound |
| `0x09` | [Resource Pack Push](#resource-pack-push) | Client-bound |
| `0x0A` | [Store Cookie](#store-cookie) | Client-bound |
| `0x0B` | [Transfer](#transfer) | Client-bound |
| `0x0C` | [Update Enabled Features](#update-enabled-features) | Client-bound |
| `0x0D` | [Update Tags](#update-tags) | Client-bound |
| `0x0E` | [Select Known Packs](#select-known-packs) | Client-bound |
| `0x0F` | [Custom Report Details](#custom-report-details) | Client-bound |
| `0x10` | [Server Links](#server-links) | Client-bound |
| `0x11` | [Clear Dialog](#clear-dialog) | Client-bound |
| `0x12` | [Show Dialog](#show-dialog) | Client-bound |
| `0x13` | [Code of Conduct](#code-of-conduct) | Client-bound |
| `0x00` | [Client Information](#client-information) | Server-bound |
| `0x01` | [Cookie Response](#cookie-response) | Server-bound |
| `0x02` | [Custom Payload (Plugin Message)](#custom-payload-plugin-message) | Server-bound |
| `0x03` | [Finish Configuration](#finish-configuration-serverbound) | Server-bound |
| `0x04` | [Keep Alive](#keep-alive) | Server-bound |
| `0x05` | [Pong](#pong) | Server-bound |
| `0x06` | [Resource Pack Response](#resource-pack-response) | Server-bound |
| `0x07` | [Select Known Packs](#select-known-packs) | Server-bound |
| `0x08` | [Custom Click Action](#custom-click-action) | Server-bound |
| `0x09` | [Accept Code of Conduct](#accept-code-of-conduct) | Server-bound |

---

## Client-bound

### Cookie Request

**Packet ID:** `0x00` · **State:** Configuration · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Key | [Identifier](./data-types#identifier) | The cookie key the server wants the client to return. |

**Semantics.** Identical in shape and intent to [Login Cookie Request](./login#cookie-request). The client replies with [Cookie Response](#cookie-response).

### Custom Payload (Plugin Message)

**Packet ID:** `0x01` · **State:** Configuration · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Channel | [Identifier](./data-types#identifier) | Namespaced channel identifier. The vanilla client recognises `minecraft:brand`; everything else is forwarded to plugins. |
| Data | [Byte Array](./data-types#byte-array) (consumes rest of packet) | Channel-specific payload. Maximum 1 048 576 bytes for clientbound. |

**Semantics.** Generic side-channel for plugin-defined messaging. The vanilla `minecraft:brand` payload carries a single UTF-8 string identifying the server software (e.g. `"vanilla"`, `"Paper"`).

### Disconnect

**Packet ID:** `0x02` · **State:** Configuration · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Reason | [NBT](./data-types#nbt) [Text Component](./text-component) | The disconnect reason, encoded as the binary network NBT form (no JSON wrapper, unlike [Login Disconnect](./login#disconnect-login)). |

**Semantics.** Server-initiated termination of the connection. Sent at any point during Configuration; the socket is closed immediately afterwards.

### Finish Configuration (clientbound)

**Packet ID:** `0x03` · **State:** Configuration · **Bound To:** Client

This packet has no fields.

**Semantics.** Terminal clientbound packet of the Configuration state. On receipt the client must finish processing all queued Configuration packets, switch its connection state to Play, and acknowledge with [Finish Configuration (serverbound)](#finish-configuration-serverbound). The server switches to Play upon receiving the acknowledgement.

### Keep Alive

**Packet ID:** `0x04` · **State:** Configuration · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| ID | [Long](./data-types#long) | Server-chosen, typically a tick counter or `System.currentTimeMillis()`. |

**Semantics.** Sent periodically (vanilla: every 15 s) to detect dead connections. The client must echo the ID back in [Keep Alive (serverbound)](#keep-alive-1) within 30 s or be disconnected with `disconnect.timeout`.

### Ping

**Packet ID:** `0x05` · **State:** Configuration · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| ID | [Int](./data-types#int) | Opaque server-chosen identifier echoed back by the client. |

**Semantics.** Latency probe distinct from Keep Alive — the client replies with [Pong](#pong) but no timeout enforcement is implied.

### Reset Chat

**Packet ID:** `0x06` · **State:** Configuration · **Bound To:** Client

This packet has no fields.

**Semantics.** Tells the client to discard any cached chat session keys / signatures from a previous Play session. Used during reconfiguration so secure-chat state from the prior server is not carried into the next one.

### Registry Data

**Packet ID:** `0x07` · **State:** Configuration · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Registry ID | [Identifier](./data-types#identifier) | The resource-key of the registry being sent (e.g. `minecraft:worldgen/biome`, `minecraft:dimension_type`, `minecraft:damage_type`). |
| Entries | [Prefixed Array](./data-types#prefixed-array) | A length-prefixed list of `(Identifier id, Optional<NBT> data)`. When `data` is absent, the entry is a *reference* to a value provided by a known pack agreed upon via [Select Known Packs](#select-known-packs); when present, it overrides or defines the entry inline as binary NBT. |

**Semantics.** Sent once per registry that the server wants to synchronise. The client merges all received Registry Data packets into its registry access and then uses them to decode subsequent registry-aware payloads (entity types, biomes, dimensions, …). See [Registries](./registries) for the per-registry NBT shapes and the `minecraft:core`/`minecraft:update` known-pack semantics.

### Resource Pack Pop

**Packet ID:** `0x08` · **State:** Configuration · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| ID | Optional [UUID](./data-types#uuid) | When present, removes the resource pack with this ID from the client's stack. When absent, removes *all* server-pushed resource packs. |

**Semantics.** Reverses a previous [Resource Pack Push](#resource-pack-push). Vanilla supports a stack of layered server packs since 1.20.3.

### Resource Pack Push

**Packet ID:** `0x09` · **State:** Configuration · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| ID | [UUID](./data-types#uuid) | A unique handle the server can later use in [Resource Pack Pop](#resource-pack-pop). |
| URL | [String](./data-types#string) | HTTP(S) URL the client downloads the pack from. |
| Hash | [String](./data-types#string) (40) | Hex-encoded SHA-1 of the pack ZIP. The client uses this for caching and integrity checking; an empty string disables the check. |
| Forced | [Boolean](./data-types#boolean) | When `true`, declining or failing to apply the pack disconnects the client. |
| Prompt Message | Optional [Text Component](./text-component) | A custom message shown in the "Server requires resource pack" dialog. |

**Semantics.** Pushes a new resource pack onto the client's pack stack. The client reports its progress via [Resource Pack Response](#resource-pack-response) (multiple times — `ACCEPTED`, `DOWNLOADED`, `SUCCESSFULLY_LOADED` / failure terminals).

### Store Cookie

**Packet ID:** `0x0A` · **State:** Configuration · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Key | [Identifier](./data-types#identifier) | The cookie key. |
| Payload | [Byte Array](./data-types#byte-array) ([VarInt](./data-types#varint)-prefixed, max 5120 bytes) | Opaque value the client persists in memory. Cleared on client shutdown. |

**Semantics.** Asks the client to remember an opaque blob keyed by Identifier. A future [Cookie Request](#cookie-request) (in either Login or Configuration) returns the same blob. Cookies survive [Transfer](#transfer), enabling session-token-style flows across servers.

### Transfer

**Packet ID:** `0x0B` · **State:** Configuration · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Host | [String](./data-types#string) | Target server hostname/IP. |
| Port | [VarInt](./data-types#varint) | Target server TCP port. |

**Semantics.** Instructs the client to disconnect from the current server and reconnect to the specified address with `Next State = 3` (Transfer) in its [Handshake](./handshake#intention-handshake). Cookies stored via [Store Cookie](#store-cookie) survive the transfer.

### Update Enabled Features

**Packet ID:** `0x0C` · **State:** Configuration · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Features | Set of [Identifier](./data-types#identifier) (length-prefixed) | The set of feature-flag IDs enabled on this server (e.g. `minecraft:vanilla`, `minecraft:bundle`, `minecraft:trade_rebalance`). |

**Semantics.** Tells the client which experimental / opt-in feature packs are active so it knows whether to render bundle items, etc. Sent before [Registry Data](#registry-data) because the registry contents depend on enabled features.

### Update Tags

**Packet ID:** `0x0D` · **State:** Configuration · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Tags | Map (length-prefixed) | Key: registry [ResourceKey](./data-types#resourcekey) (encoded as Identifier). Value: a `NetworkPayload` — itself a length-prefixed map of `Identifier → VarInt[]` mapping each tag name to the list of in-registry numeric IDs it contains. |

**Semantics.** Sent after [Registry Data](#registry-data). The client uses these to resolve tag references in subsequent packets (block tags, item tags, fluid tags, biome tags, damage-type tags, …). See [Tags](./tags).

### Select Known Packs

**Packet ID:** `0x0E` · **State:** Configuration · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Known Packs | [Prefixed Array](./data-types#prefixed-array) of `KnownPack` | Each entry is `{ namespace: String, id: String, version: String }`. The vanilla server sends a single entry: `{"minecraft", "core", <gameVersion>}`. |

**Semantics.** Sent before [Registry Data](#registry-data). Lists the data packs whose contents the *server* assumes the client already has bundled. The client replies with [Select Known Packs (serverbound)](#select-known-packs-1) listing the subset it actually recognises; for those packs the server omits the entry data in subsequent Registry Data packets, drastically shrinking the join handshake.

### Custom Report Details

**Packet ID:** `0x0F` · **State:** Configuration · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Details | Map<[String](./data-types#string), [String](./data-types#string)> | Up to 32 key/value pairs (key ≤ 128 chars, value ≤ 4096 chars) added to client crash reports captured while connected to this server. |

**Semantics.** Aids debugging by injecting server-supplied diagnostics into any crash report the client subsequently generates.

### Server Links

**Packet ID:** `0x10` · **State:** Configuration · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Links | [Prefixed Array](./data-types#prefixed-array) of `(Label, URL)` | Each label is either an enum tag (`Bug Report`, `Community Guidelines`, `Support`, `Status`, `Feedback`, `Community`, `Website`, `Forums`, `News`, `Announcements`) or a free-form [Text Component](./text-component); the URL is a String. |

**Semantics.** Populates the entries shown in the in-game pause menu's "Server Links" submenu.

### Clear Dialog

**Packet ID:** `0x11` · **State:** Configuration · **Bound To:** Client

This packet has no fields.

**Semantics.** Closes the dialog window opened by the most recent [Show Dialog](#show-dialog).

### Show Dialog

**Packet ID:** `0x12` · **State:** Configuration · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Dialog | Inline `Dialog` ([NBT](./data-types#nbt)) | The full dialog definition: title, body components, list of buttons / inputs and their actions. In Configuration the dialog is sent inline (context-free) because no registry context is available; in Play it may instead reference a registered dialog by ID. |

**Semantics.** Opens a server-defined dialog UI on the client. User interactions are reported back via [Custom Click Action](#custom-click-action). See the dialog system documentation under [Registries](./registries#dialog).

### Code of Conduct

**Packet ID:** `0x13` · **State:** Configuration · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Text | [String](./data-types#string) | The code-of-conduct text the player must accept before joining. |

**Semantics.** Asks the client to display a code-of-conduct screen. The client must reply with [Accept Code of Conduct](#accept-code-of-conduct) before the server will send [Finish Configuration](#finish-configuration-clientbound).

---

## Server-bound

### Client Information

**Packet ID:** `0x00` · **State:** Configuration · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Locale | [String](./data-types#string) (16) | The client's display language tag, e.g. `en_us`. |
| View Distance | [Byte](./data-types#byte) | Render distance the client has configured, in chunks. The server should clamp this to its own `view-distance` setting. |
| Chat Mode | [VarInt](./data-types#varint) Enum | `0` Full, `1` System messages only, `2` Hidden. |
| Chat Colors | [Boolean](./data-types#boolean) | Whether the client wants chat messages to include colour formatting. |
| Displayed Skin Parts | [Unsigned Byte](./data-types#unsigned-byte) | Bitmask: cape (0x01), jacket (0x02), left sleeve (0x04), right sleeve (0x08), left pants (0x10), right pants (0x20), hat (0x40); bit 7 unused. |
| Main Hand | [VarInt](./data-types#varint) Enum | `0` Left, `1` Right. |
| Text Filtering Enabled | [Boolean](./data-types#boolean) | `true` if the client has parental text filtering enabled (typically only Bedrock-linked accounts). |
| Allows Server Listing | [Boolean](./data-types#boolean) | When `false`, the client opts out of being shown in `players.sample` of [Status Response](./status#status-response). |
| Particle Status | [VarInt](./data-types#varint) Enum | `0` All, `1` Decreased, `2` Minimal. |

**Semantics.** Sent by the client immediately after entering Configuration, and re-sent in Play (with the same packet, in the Play state) whenever the user changes one of these settings. The server should persist the values per connection.

### Cookie Response

**Packet ID:** `0x01` · **State:** Configuration · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Key | [Identifier](./data-types#identifier) | Echoes the key from [Cookie Request](#cookie-request). |
| Payload | Optional [Byte Array](./data-types#byte-array) ([VarInt](./data-types#varint)-prefixed, max 5120 bytes) | Stored cookie value, or absent if the client has nothing for that key. |

**Semantics.** Reply to [Cookie Request](#cookie-request). Identical shape to the Login version.

### Custom Payload (Plugin Message)

**Packet ID:** `0x02` · **State:** Configuration · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Channel | [Identifier](./data-types#identifier) | Channel ID. |
| Data | [Byte Array](./data-types#byte-array) (consumes rest of packet, max 32767 bytes) | Channel-specific payload. |

**Semantics.** Generic plugin side-channel from client to server. Smaller maximum payload size than the clientbound variant.

### Finish Configuration (serverbound)

**Packet ID:** `0x03` · **State:** Configuration · **Bound To:** Server

This packet has no fields.

**Semantics.** Acknowledgement of [Finish Configuration (clientbound)](#finish-configuration-clientbound). Terminal serverbound packet of the Configuration state — both sides switch to Play immediately.

### Keep Alive

**Packet ID:** `0x04` · **State:** Configuration · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| ID | [Long](./data-types#long) | The exact ID echoed from the most recent [Keep Alive (clientbound)](#keep-alive). |

**Semantics.** Reply that resets the server's keep-alive timer. A mismatched ID is a protocol error and should disconnect the client.

### Pong

**Packet ID:** `0x05` · **State:** Configuration · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| ID | [Int](./data-types#int) | Echoes the ID from [Ping](#ping). |

**Semantics.** Reply to [Ping](#ping). Used for latency measurement; not subject to a hard timeout.

### Resource Pack Response

**Packet ID:** `0x06` · **State:** Configuration · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| ID | [UUID](./data-types#uuid) | Identifies the [Resource Pack Push](#resource-pack-push) this status refers to. |
| Result | [VarInt](./data-types#varint) Enum | `0` Successfully loaded, `1` Declined, `2` Failed download, `3` Accepted, `4` Downloaded, `5` Invalid URL, `6` Failed reload, `7` Discarded. `Accepted` and `Downloaded` are progress events; the rest are terminal. |

**Semantics.** Sent multiple times for each push: typically `Accepted` → `Downloaded` → `Successfully loaded`. If `Forced` was set and a terminal failure result is reported, the server should disconnect the client.

### Select Known Packs

**Packet ID:** `0x07` · **State:** Configuration · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Known Packs | [Prefixed Array](./data-types#prefixed-array) of `KnownPack` (max 64 entries) | The subset of the server-advertised packs that the client actually has bundled. |

**Semantics.** Reply to [Select Known Packs (clientbound)](#select-known-packs). Determines which Registry Data entries the server may send by reference instead of inline.

### Custom Click Action

**Packet ID:** `0x08` · **State:** Configuration · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| ID | [Identifier](./data-types#identifier) | The action identifier defined by the server-supplied dialog. |
| Payload | Optional [NBT](./data-types#nbt) [Tag](./data-types#nbt) | Untrusted (size-capped: 32 KiB tag, 64 KiB length-prefix) payload describing the inputs the user filled in. |

**Semantics.** Sent in response to a [Show Dialog](#show-dialog) when the user clicks a button bound to a custom action. Also reachable from Play.

### Accept Code of Conduct

**Packet ID:** `0x09` · **State:** Configuration · **Bound To:** Server

This packet has no fields.

**Semantics.** Sent by the client when the user accepts the code of conduct displayed by [Code of Conduct](#code-of-conduct). The server should refuse to send [Finish Configuration](#finish-configuration-clientbound) until this packet has been received (when a code of conduct is configured).
