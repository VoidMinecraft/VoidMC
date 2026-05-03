# Entities

This page is the thematic overview of how the protocol manages entities. The byte-level layout of every packet listed here lives in [Play — Clientbound](./play-clientbound) and [Play — Serverbound](./play-serverbound); this page only documents the conventions and cross-cutting semantics. Synced data fields ("entity metadata") are described in [./entity-metadata](./entity-metadata).

The server is fully authoritative over entity state. Clients receive a stream of small mutation packets that reference entities by their server-assigned **Entity ID** (a VarInt; 0 is reserved for "no entity" in some packets). UUIDs are only sent at spawn time and for player-info bookkeeping.

## Lifecycle

| ID | Packet | Direction | Purpose |
|----|--------|-----------|---------|
| `0x01` | [Add Entity](./play-clientbound#0x01---add-entity) | Client-bound | Spawn an entity (any type, including the player's own object form when seen by others). |
| `0x4D` | [Remove Entities](./play-clientbound#0x4d---remove-entities) | Client-bound | Despawn one or more entities by ID. |
| `0x63` | [Set Entity Data](./play-clientbound#0x63---set-entity-data) | Client-bound | Push synced data fields (see [./entity-metadata](./entity-metadata)). |
| `0x35` | [Move Entity Pos](./play-clientbound#0x35---move-entity-pos) | Client-bound | Small relative position movement (delta encoded). |
| `0x36` | [Move Entity Pos Rot](./play-clientbound#0x36---move-entity-pos-rot) | Client-bound | Small relative position + rotation movement (delta encoded). |
| `0x38` | [Move Entity Rot](./play-clientbound#0x38---move-entity-rot) | Client-bound | Small relative rotation. |
| `0x7D` | [Teleport Entity](./play-clientbound#0x7d---teleport-entity) | Client-bound | Absolute reposition with optional relative-flags. |
| `0x23` | [Entity Position Sync](./play-clientbound#0x23---entity-position-sync) | Client-bound | Authoritative resync of position+velocity (no animation). |
| `0x53` | [Rotate Head](./play-clientbound#0x53---rotate-head) | Client-bound | Updates only the head yaw. |
| `0x65` | [Set Entity Motion](./play-clientbound#0x65---set-entity-motion) | Client-bound | Sets velocity for client-side prediction. |
| `0x22` | [Entity Event](./play-clientbound#0x22---entity-event) | Client-bound | Status code (one byte) — death animations, totem of undying, etc. |
| `0x19` | [Damage Event](./play-clientbound#0x19---damage-event) | Client-bound | Modern signed damage notification (replaces legacy `Animate(hurt)`). |
| `0x2A` | [Hurt Animation](./play-clientbound#0x2a---hurt-animation) | Client-bound | Pure visual hurt direction. |
| `0x02` | [Animate](./play-clientbound#0x02---animate) | Client-bound | One of: swing, leave bed, critical effect, magic critical, swing offhand. |
| `0x7C` | [Take Item Entity](./play-clientbound#0x7c---take-item-entity) | Client-bound | Animates an item being collected by an entity. |
| `0x6B` | [Set Passengers](./play-clientbound#0x6b---set-passengers) | Client-bound | Replaces the passenger list of a vehicle. |
| `0x39` | [Move Vehicle](./play-clientbound#0x39---move-vehicle) | Client-bound | Server-driven vehicle reposition. |
| `0x22` | [Move Vehicle](./play-serverbound#0x22---move-vehicle) | Server-bound | Player-controlled vehicle movement. |
| `0x74` | [Sound Entity](./play-clientbound#0x74---sound-entity) | Client-bound | Plays a sound attached to an entity. |
| `0x46` | [Player Info Update](./play-clientbound#0x46---player-info-update) | Client-bound | Manages tablist + per-player chat session keys. |
| `0x45` | [Player Info Remove](./play-clientbound#0x45---player-info-remove) | Client-bound | Drop tablist entries by UUID. |

## Identifiers

- **Entity ID** — VarInt, server-assigned, unique within the world. Reused after despawn. `0` is reserved for "no entity" in some optional fields (e.g. damage source IDs, where the wire value is `entity_id + 1`).
- **Entity UUID** — sent only by [Add Entity](./play-clientbound#0x01---add-entity) and the player-info packets. Used for player identity and chat-signature correlation, never for routine packet addressing.

## Angle encoding

Entity rotation on the wire uses byte-encoded angles for spawn / movement packets and float-degrees for absolute teleports.

- **Byte angle** (`Pitch`, `Yaw`, `Head yaw`): `byte = floor(degrees * 256 / 360)`. One unit ≈ 1.406°.
- **Float angle** ([Teleport Entity](./play-clientbound#0x7d---teleport-entity), [Entity Position Sync](./play-clientbound#0x23---entity-position-sync), [Hurt Animation](./play-clientbound#0x2a---hurt-animation), vehicle moves): plain degrees as IEEE-754 float.

For non-living entities the head yaw is normally equal to the body yaw.

## Position: absolute vs delta

There are three encodings in use for entity position:

1. **Absolute Double**, used by [Add Entity](./play-clientbound#0x01---add-entity), [Teleport Entity](./play-clientbound#0x7d---teleport-entity), [Entity Position Sync](./play-clientbound#0x23---entity-position-sync) and the vehicle-move packets. Plain `Double` per axis.
2. **Delta Short**, used by [Move Entity Pos](./play-clientbound#0x35---move-entity-pos) and [Move Entity Pos Rot](./play-clientbound#0x36---move-entity-pos-rot). Encodes `current - previous` in 1/4096 of a block (`short = Δblocks * 4096`). A delta packet is only valid while the entity has not moved more than ±8 blocks since the last absolute teleport (`±8 blocks * 4096 = ±32768` saturates the short). When the budget is exceeded, or to defeat accumulated rounding error, the server sends a full Teleport Entity instead.
3. **Velocity Short**, used in [Add Entity](./play-clientbound#0x01---add-entity) and [Set Entity Motion](./play-clientbound#0x65---set-entity-motion). Each axis encodes `(blocks per tick) * 8000` clipped to a signed short.

[Teleport Entity](./play-clientbound#0x7d---teleport-entity) carries a *Relative flags* bitset that lets a single packet mix absolute and delta semantics per axis (used for elytra / boat resync without snapping):

| Bit | Meaning |
|----:|---------|
| 0 | X is relative |
| 1 | Y is relative |
| 2 | Z is relative |
| 3 | Yaw is relative |
| 4 | Pitch is relative |
| 5 | Velocity X is relative |
| 6 | Velocity Y is relative |
| 7 | Velocity Z is relative |
| 8 | Rotate velocity by the new yaw/pitch |

## Damage and animations

Modern damage uses [Damage Event](./play-clientbound#0x19---damage-event) (signed source identifiers, optional source position) plus [Hurt Animation](./play-clientbound#0x2a---hurt-animation) for the camera/model flinch direction. The legacy `Animate(hurt)` flag is no longer used in 26.1.2 — Animate now only carries the swing / bed / critical / magic-critical / off-hand-swing codes.

[Entity Event](./play-clientbound#0x22---entity-event) is a generic catch-all that uses a **32-bit Int** entity ID (not a VarInt — kept for protocol-historical reasons) and a single byte of event code (`2` = hurt legacy, `3` = death, `35` = totem, `60` = explode, etc.).

## Add Entity `Data` field

The numeric `Type` field in [Add Entity](./play-clientbound#0x01---add-entity) indexes into the `minecraft:entity_type` registry, whose IDs are part of the protocol contract for 26.1.2. The auxiliary `Data` integer is type-dependent; the most common uses are:

| Entity type | `Data` meaning |
|-------------|----------------|
| `falling_block` | Block-state numeric ID. |
| `item_frame`, `glow_item_frame`, `painting` | Hanging facing direction (0=down, 1=up, 2=north, 3=south, 4=west, 5=east). |
| `arrow`, `spectral_arrow`, `trident` | Shooter entity ID + 1 (0 = none). |
| `fishing_bobber` | Owner player entity ID. |
| `fireball`, `small_fireball`, `dragon_fireball`, `wither_skull` | Shooter entity ID + 1. |
| `experience_orb` | XP value. |
| All others | `0`. |

Most entity-specific state (variant, age, color, pose, nameplate, etc.) lives in the synced data fields delivered by [Set Entity Data](./play-clientbound#0x63---set-entity-data); see [./entity-metadata](./entity-metadata).

## Player as entity

A player is also an entity, but tablist and chat-session bookkeeping is independent of its entity ID. [Player Info Update](./play-clientbound#0x46---player-info-update) carries an action bitset (add player, initialize chat, update gamemode, update listed, update latency, update display name, update list priority, update show-hat) plus per-action payloads keyed by player UUID. [Player Info Remove](./play-clientbound#0x45---player-info-remove) is just a list of UUIDs to drop.

These packets are how the client learns of a player's chat session public key (see [./chat](./chat)) and gamemode changes that occur on other clients.

> Source: net/minecraft/network/protocol/game/ClientboundAddEntityPacket.java, ClientboundMoveEntityPacket.java, ClientboundTeleportEntityPacket.java, ClientboundEntityPositionSyncPacket.java, ClientboundSetEntityDataPacket.java, ClientboundRemoveEntitiesPacket.java, ClientboundRotateHeadPacket.java, ClientboundSetEntityMotionPacket.java, ClientboundEntityEventPacket.java, ClientboundDamageEventPacket.java, ClientboundHurtAnimationPacket.java, ClientboundAnimatePacket.java, ClientboundTakeItemEntityPacket.java, ClientboundSetPassengersPacket.java, ClientboundMoveVehiclePacket.java, ClientboundSoundEntityPacket.java, ClientboundPlayerInfoUpdatePacket.java, ClientboundPlayerInfoRemovePacket.java, net/minecraft/world/entity/Entity.java, net/minecraft/world/entity/EntityType.java.
