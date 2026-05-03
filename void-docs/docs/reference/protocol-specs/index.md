# Protocol Specifications

This section documents the official Minecraft network protocol, version by version. It is a reference for anyone implementing a Minecraft-compatible server or client — not a description of how Void implements it. Void is one consumer of this specification.

Each version page describes the wire format, the connection lifecycle, every packet, and the surrounding subsystems (registries, tags, chunks, entities, commands, chat) needed to bring up a working server.

## Supported Versions

| Version | Minecraft | Status |
|---------|-----------|--------|
| [26.1.2](./v26.1.2/) | 26.1.2 | Current |

> Mojang switched the Minecraft version naming scheme: there is no longer a `1.x.y` prefix. The Minecraft version, the Paper `mcVersion`, and this spec all use the same `26.1.2` identifier.

New versions will be added as separate sub-sections so older specifications remain available for reference.

## Conventions

- **Direction.** *Clientbound* (CB) packets travel from the server to the client. *Serverbound* (SB) packets travel from the client to the server.
- **Packet IDs** are written in hexadecimal (e.g. `0x01`) and are scoped per state and per direction.
- **Field tables** use the column `Notes` to describe what each field represents and any non-obvious encoding rule.
- **Source of truth.** Field layouts and IDs are derived from the vanilla Minecraft sources as patched by [PaperMC](https://papermc.io). When the spec is ambiguous, Paper behavior is used as the canonical reference.
