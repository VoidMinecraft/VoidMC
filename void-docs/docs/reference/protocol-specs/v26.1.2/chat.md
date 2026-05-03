# Chat System

Minecraft 1.20 introduced — and 1.21+ refined — a hybrid chat model. Messages may flow as **signed player chat** (cryptographically tied to a Mojang-issued profile key, allowing third-party moderation), as **unsigned disguised chat** (server-rendered as if from a player), or as **system chat** (no sender). All player-visible text is always rendered through a **Chat Type**, which provides the translation key and parameter set used by the client.

This page specifies the chat-related packets in 26.1.2. Chat-type registry shape is covered in [./registries](./registries).

## Concepts

- **Chat Type**: a registry entry (`minecraft:chat_type`) holding a chat-decoration and a narration-decoration. Each decoration has a translation key (e.g. `chat.type.text`), a parameter list (`["sender", "content"]`), and an optional style.
- **Bound Chat Type**: a chat type plus its bound *sender name* and optional *target name* — the data needed to render an actual message header.
- **Message Signature**: a fixed 256-byte signature over the message body, signable by the client's session public key.
- **Message Signature Cache / ID**: signatures are interned per-connection so they can be referenced by a small VarInt instead of resending the 256 bytes.
- **Last Seen Messages**: the rolling window (≤ 20 entries) of message signatures the sender had observed when authoring the message; included in the signed payload to bind context.
- **Chat Session**: a per-connection `(session UUID, public key)` pair. Established serverbound; required before the client may send signed messages.

## Clientbound packets

### Player Chat

A signed message originating from a player.

| Field | Type | Notes |
|-------|------|-------|
| Global index | [VarInt](./data-types#varint) | Server's monotonic counter for the message stream (used for `Chat Acknowledgement`). |
| Sender UUID | [UUID](./data-types#uuid) | Profile UUID of the sender. |
| Index | [VarInt](./data-types#varint) | Sender's per-session index. |
| Has signature | [Boolean](./data-types#boolean) | |
| Signature | byte[256] | Only when `Has signature` is true. |
| Body | sub-payload | See [Signed message body](#signed-message-body). |
| Has unsigned content | [Boolean](./data-types#boolean) | |
| Unsigned content | Component | Server-overridden display content (when the server rewrites the rendered message); only when present. |
| Filter mask | sub-payload | See [Filter mask](#filter-mask). |
| Chat type | Bound chat type | See [Bound chat type](#bound-chat-type). |

#### Signed message body

| Field | Type | Notes |
|-------|------|-------|
| Content | [String](./data-types#string) (≤ 256) | Plain UTF-8 message text. |
| Timestamp | [Long](./data-types#long) | Milliseconds since epoch. |
| Salt | [Long](./data-types#long) | Random salt used in the signature. |
| Last seen | Array | VarInt count + per-entry `(VarInt id, Optional byte[256])`. `id` is `cache_id + 1`; if `id == 0`, the full 256-byte signature is inlined; otherwise the entry references the cached signature at `cache_id`. |

#### Filter mask

| Field | Type | Notes |
|-------|------|-------|
| Type | [VarInt](./data-types#varint) enum | 0 = `PASS_THROUGH`, 1 = `FULLY_FILTERED`, 2 = `PARTIALLY_FILTERED`. |
| Bits | [BitSet](./data-types#bitset) | Only present when `Type == 2`. Each set bit marks a UTF-16 code unit of `Content` that the moderation provider flagged. |

#### Bound chat type

| Field | Type | Notes |
|-------|------|-------|
| Chat type | Holder | VarInt `id + 1` referring to a `minecraft:chat_type` entry, or `0` followed by an inline `ChatType` NBT compound. |
| Sender name | Component | Display component for the sender slot. |
| Has target name | [Boolean](./data-types#boolean) | |
| Target name | Component | Only present when `Has target name` is true (private message recipient, team chat, etc.). |

### Disguised Chat

An unsigned, server-authored message the client should render as if it were a player message.

| Field | Type | Notes |
|-------|------|-------|
| Message | Component | Already-formatted message content. |
| Chat type | Bound chat type | Same shape as in Player Chat. |

### System Chat

| Field | Type | Notes |
|-------|------|-------|
| Content | Component | |
| Overlay | [Boolean](./data-types#boolean) | If true, render in the action-bar slot above the hotbar instead of the chat box. |

### Delete Chat

Asks the client to remove a previously delivered Player Chat message from its history.

| Field | Type | Notes |
|-------|------|-------|
| Message signature | Packed signature | `(VarInt id, Optional byte[256])`: `id == 0` means full inline signature follows; otherwise `id - 1` indexes the message-signature cache. |

### Chat Suggestions

Adds, replaces, or removes a list of plain-text completions surfaced in the client's chat tab-completion popup (independent of command argument suggestions).

| Field | Type | Notes |
|-------|------|-------|
| Action | [VarInt](./data-types#varint) enum | 0 = add, 1 = remove, 2 = set. |
| Entries | Array of [String](./data-types#string) | Length-prefixed list. |

### Custom Chat Completions

A server-extensible variant of `Chat Suggestions` for things like player-name completion. Same shape as `Chat Suggestions`.

## Serverbound packets

### Chat Session Update

Establishes (or rotates) the client's chat session.

| Field | Type | Notes |
|-------|------|-------|
| Session UUID | [UUID](./data-types#uuid) | Client-generated session identifier. |
| Expires at | [Long](./data-types#long) | Public key expiry as milliseconds since epoch. |
| Public key | byte[] (≤ ~512) | DER-encoded RSA public key. |
| Key signature | byte[] (≤ 4096) | Mojang's signature over the key + sender UUID + expiry. |

The server validates the key signature against the Mojang services key; if valid, future signed `Chat` packets from this client are decoded with this key.

### Chat (signed)

(Mentioned for completeness — see [./play-serverbound](./play-serverbound) for the full layout.) Carries the message text, salt, timestamp, optional 256-byte signature, the sender's last-seen window, and the offset acknowledgement.

### Chat Command (signed)

Same shape as Chat, but with a parsed command path: each signable `String` argument is sent with its own per-argument signature.

### Chat Acknowledgement

| Field | Type | Notes |
|-------|------|-------|
| Offset | [VarInt](./data-types#varint) | Number of additional messages the client has acknowledged since its last ack. The server uses this to advance the client's last-seen window without retransmitting signatures. |

## Chat type registry recap

A `minecraft:chat_type` NBT compound (sent via [Registry Data](./registries)) is:

| Field | Type | Notes |
|-------|------|-------|
| `chat` | [Compound](./data-types#compound) | Decoration for the chat box. |
| `narration` | [Compound](./data-types#compound) | Decoration for screen-reader narration. |

Each decoration:

| Field | Type | Notes |
|-------|------|-------|
| `translation_key` | [String](./data-types#string) | e.g. `"chat.type.text"`, `"chat.type.announcement"`. |
| `parameters` | List of [String](./data-types#string) | Subset of `["sender", "target", "content"]`, in render order. |
| `style` | [Compound](./data-types#compound) (optional) | Style applied to the rendered string (color, italic, etc.). |

When the client receives a Player Chat or Disguised Chat with bound chat type `T` and parameters `(sender, content[, target])`, it builds the displayed component by translating `T.chat.translation_key` with those parameters and applying `T.chat.style`.

> Source: net/minecraft/network/protocol/game/ClientboundPlayerChatPacket.java, ClientboundDisguisedChatPacket.java, ClientboundSystemChatPacket.java, ClientboundDeleteChatPacket.java, ClientboundChatSuggestionsPacket.java, ClientboundCustomChatCompletionsPacket.java, ServerboundChatSessionUpdatePacket.java, ServerboundChatAckPacket.java, ServerboundChatPacket.java, ServerboundChatCommandSignedPacket.java, net/minecraft/network/chat/ChatType.java, ChatTypeDecoration.java, MessageSignature.java, SignedMessageBody.java, LastSeenMessages.java, FilterMask.java, RemoteChatSession.java, net/minecraft/world/entity/player/ProfilePublicKey.java.
