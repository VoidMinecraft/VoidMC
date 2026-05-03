# Protocol 26.1.2 — Overview

This section specifies the Minecraft Java Edition network protocol as shipped with Minecraft 26.1.2 (`mcVersion=26.1.2` in PaperMC). Mojang dropped the `1.x.y` versioning scheme; the version this spec targets is simply `26.1.2`. It describes the wire format, every packet of every state, and the subsystems a server must implement to serve a vanilla client.

## How to read this section

Start with the foundations, then move to the state-by-state packet listings, and finally the cross-cutting subsystems.

1. **Foundations**
   - [Data Types](./data-types) — primitive encodings (VarInt, NBT, Position…) used everywhere below.
   - [Text Component](./text-component) — the rich-text format used in chat, titles, item names.
   - [Slot / ItemStack](./slot) — items on the wire, including data components.
   - [Particle](./particle) — particle descriptors used in level events.
   - [Connection Lifecycle](./connection-lifecycle) — state machine and packet ordering.
2. **States** (in chronological order of the connection)
   - [Handshake](./handshake)
   - [Status](./status)
   - [Login](./login)
   - [Configuration](./configuration)
   - [Play — Clientbound](./play-clientbound)
   - [Play — Serverbound](./play-serverbound)
3. **Subsystems**
   - [Registries](./registries)
   - [Tags](./tags)
   - [Chunks & Lighting](./chunks)
   - [Entities](./entities)
   - [Entity Metadata](./entity-metadata)
   - [Commands](./commands)
   - [Chat System](./chat)

## Framing

Every packet on the wire is framed as:

```
+----------------------+----------------------+------------------+
| Length (VarInt)      | Packet ID (VarInt)   | Payload (bytes)  |
+----------------------+----------------------+------------------+
```

- **Length** is the total length, in bytes, of `Packet ID` + `Payload`. It does **not** include itself.
- **Packet ID** identifies the packet within the current state and direction.
- **Payload** is the packet's body, encoded according to the field list in the relevant state page.

Once the connection enters the *compression* mode (see [Login](./login#set-compression)), the framing changes:

```
+----------------+----------------------+------------------------------+
| Packet Length  | Data Length (VarInt) | Compressed (ID + Payload)    |
+----------------+----------------------+------------------------------+
```

- If `Data Length` is `0`, the rest of the packet is uncompressed.
- Otherwise, `Data Length` is the uncompressed size, and the remainder is zlib-compressed.

Encryption, when enabled, is applied as an outer AES/CFB8 layer over the framed bytes.
