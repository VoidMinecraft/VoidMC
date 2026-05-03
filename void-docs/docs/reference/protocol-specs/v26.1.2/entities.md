# Entities

This page specifies the wire format used to manage entities in the [Play state](./play-clientbound). Per-entity *metadata* (the synced data fields) lives in its own page, see [./entity-metadata](./entity-metadata).

The server is fully authoritative over entity state. Clients receive a stream of small mutation packets that reference entities by their server-assigned **Entity ID** (a VarInt; 0 is reserved for "no entity" in some packets). UUIDs are only sent at spawn time and for player-info bookkeeping.

## Packet Index

| ID | Name | Direction | Purpose |
|----|------|-----------|---------|
| `0x01` | [Add Entity](#0x01---add-entity) | Client-bound | Spawn an entity (any type, including the player's own object form when seen by others). |
| `0x02` | [Animate](#0x02---animate) | Client-bound | One of: swing, leave bed, critical effect, magic critical, swing offhand. |
| `0x19` | [Damage Event](#0x19---damage-event) | Client-bound | Modern signed damage notification (replaces legacy `Animate(hurt)`). |
| `0x22` | [Entity Event](#0x22---entity-event) | Client-bound | Status code (one byte) — death animations, totem of undying, etc. |
| `0x23` | [Entity Position Sync](#0x23---entity-position-sync) | Client-bound | Authoritative resync of position+velocity (no animation). |
| `0x2A` | [Hurt Animation](#0x2a---hurt-animation) | Client-bound | Pure visual hurt direction. |
| `0x35` | [Move Entity Pos](#0x35---move-entity-pos) | Client-bound | Small relative position movement. |
| `0x36` | [Move Entity Pos Rot](#0x36---move-entity-pos-rot) | Client-bound | Small relative position + rotation movement. |
| `0x38` | [Move Entity Rot](#0x38---move-entity-rot) | Client-bound | Small relative rotation. |
| `0x39` | [Move Vehicle (clientbound)](#0x39---move-vehicle-clientbound) | Client-bound | Server-driven vehicle reposition. |
| `0x45` | [Player Info Remove](#0x45---player-info-remove) | Client-bound | Drop tablist entries by UUID. |
| `0x46` | [Player Info Update](#0x46---player-info-update) | Client-bound | Manage tablist + per-player chat session keys. |
| `0x4D` | [Remove Entities](#0x4d---remove-entities) | Client-bound | Despawn one or more entities by ID. |
| `0x53` | [Rotate Head](#0x53---rotate-head) | Client-bound | Updates only the head yaw. |
| `0x63` | [Set Entity Data](#0x63---set-entity-data) | Client-bound | Push synced data fields (see [./entity-metadata](./entity-metadata)). |
| `0x65` | [Set Entity Motion](#0x65---set-entity-motion) | Client-bound | Sets velocity for client-side prediction. |
| `0x6B` | [Set Passengers](#0x6b---set-passengers) | Client-bound | Replaces the passenger list of a vehicle. |
| `0x74` | [Sound Entity](#0x74---sound-entity) | Client-bound | Plays a sound attached to an entity. |
| `0x7C` | [Take Item Entity](#0x7c---take-item-entity) | Client-bound | Animates an item being collected by an entity. |
| `0x7D` | [Teleport Entity](#0x7d---teleport-entity) | Client-bound | Absolute reposition with optional relative-flags. |
| `0x22` | [Move Vehicle (serverbound)](#0x22---move-vehicle-serverbound) | Server-bound | Player-controlled vehicle movement. |

> Full clientbound and serverbound packet IDs come from [Play — Clientbound](./play-clientbound) and [Play — Serverbound](./play-serverbound). This page focuses on the entity-management subset; the bytes-on-wire layouts below match those references.

---

## 0x01 - Add Entity

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types#varint) | Server-assigned. Unique within the world. |
| Entity UUID | [UUID](./data-types#uuid) | Persistent identifier; used for player + chat correlation. |
| Type | [VarInt](./data-types#varint) | Numeric ID into the `minecraft:entity_type` registry. |
| X / Y / Z | [Double](./data-types#double) | World coordinates. |
| Pitch (xRot) | [Byte](./data-types#byte) | Angle in 1/256ths of a turn (`byte = floor(degrees * 256 / 360)`). |
| Yaw (yRot) | [Byte](./data-types#byte) | Same encoding. |
| Head yaw (yHeadRot) | [Byte](./data-types#byte) | Same encoding; usually equal to yaw for non-living entities. |
| Data | [VarInt](./data-types#varint) | Type-dependent extra integer (e.g. block ID for falling block, item entity slot index, projectile owner ID). |
| Velocity X / Y / Z | [Short](./data-types#short) | Each axis encodes `(blocks per tick) * 8000` clipped to a signed short. |

The `Data` field semantics depend on the entity type. For most entity types it is `0`. Notable users include `falling_block` (block state ID), `item_display` / `block_display` (display identifier), arrows and other projectiles (shooter entity ID + 1; 0 means "no shooter"). See [Entity types and `Data`](#entity-types-and-data) below for the full table.

## 0x02 - Animate

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types#varint) | |
| Animation | UByte | 0 = swing main hand, 2 = leave bed, 3 = swing off hand, 4 = critical effect, 5 = magic critical effect. |

## 0x19 - Damage Event

Signed/typed damage notification used for hurt animation + sound + camera direction.

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types#varint) | Damaged entity. |
| Source type | [VarInt](./data-types#varint) | ID into the `minecraft:damage_type` registry. |
| Source cause ID | [VarInt](./data-types#varint) | Direct cause entity ID + 1; 0 = none. |
| Source direct ID | [VarInt](./data-types#varint) | Indirect cause entity ID + 1; 0 = none. |
| Has source position | [Boolean](./data-types#boolean) | |
| Source X / Y / Z | [Double](./data-types#double) | Only when `Has source position` is true. |

## 0x22 - Entity Event

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [Int](./data-types#int) | (32-bit Int — not VarInt; legacy wire shape.) |
| Event ID | [Byte](./data-types#byte) | Status code. See `EntityEvent` for the full enumeration (e.g. 2 = hurt, 3 = death, 35 = totem, 60 = explode). |

## 0x23 - Entity Position Sync

Plain authoritative override (no smoothing on the client). Same field layout as [Teleport Entity](#0x7d---teleport-entity) minus the relative flags.

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types#varint) | |
| Position X / Y / Z | [Double](./data-types#double) | |
| Velocity X / Y / Z | [Double](./data-types#double) | |
| Yaw | [Float](./data-types#float) | Degrees. |
| Pitch | [Float](./data-types#float) | Degrees. |
| On ground | [Boolean](./data-types#boolean) | |

## 0x2A - Hurt Animation

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types#varint) | |
| Yaw | [Float](./data-types#float) | Direction (degrees) the camera/model should flinch from. |

## Movement: delta encoding

The three Move Entity * packets below all use **delta encoding** for position. Each axis encodes `current - previous` in 1/4096 of a block (`short = (Δblocks * 4096)`). A delta packet is only valid while the entity has not moved more than ±8 blocks since the last absolute teleport (because `±8 blocks * 4096 = ±32768` saturates the short). If a movement exceeds that range, or to defeat accumulated rounding error, the server sends [Teleport Entity](#0x7d---teleport-entity) instead.

## 0x35 - Move Entity Pos

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types#varint) | |
| Δx, Δy, Δz | [Short](./data-types#short) ×3 | Delta-encoded. |
| On ground | [Boolean](./data-types#boolean) | |

## 0x36 - Move Entity Pos Rot

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types#varint) | |
| Δx, Δy, Δz | [Short](./data-types#short) ×3 | Delta-encoded. |
| Yaw | [Byte](./data-types#byte) | 1/256ths of a turn. |
| Pitch | [Byte](./data-types#byte) | 1/256ths of a turn. |
| On ground | [Boolean](./data-types#boolean) | |

## 0x38 - Move Entity Rot

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types#varint) | |
| Yaw | [Byte](./data-types#byte) | |
| Pitch | [Byte](./data-types#byte) | |
| On ground | [Boolean](./data-types#boolean) | |

## 0x39 - Move Vehicle (clientbound)

Sent by the server to keep a player-controlled vehicle in sync. See also [Move Vehicle (serverbound)](#0x22---move-vehicle-serverbound).

| Field | Type | Notes |
|-------|------|-------|
| Position X / Y / Z | [Double](./data-types#double) | |
| Yaw | [Float](./data-types#float) | |
| Pitch | [Float](./data-types#float) | |

## 0x45 - Player Info Remove

A list of UUIDs to drop from the tablist.

| Field | Type | Notes |
|-------|------|-------|
| UUIDs | [Prefixed Array](./data-types#prefixed-array) of [UUID](./data-types#uuid) | Players to remove. |

## 0x46 - Player Info Update

Carries an action bitset (add player, initialize chat, update gamemode, update listed, update latency, update display name, update list priority, update show-hat) plus per-action payloads keyed by player UUID.

These packets are how the client learns of a player's chat session public key (see [./chat](./chat)) and gamemode changes that occur on other clients.

## 0x4D - Remove Entities

| Field | Type | Notes |
|-------|------|-------|
| Entity IDs | [Prefixed Array](./data-types#prefixed-array) of [VarInt](./data-types#varint) | Entities to despawn. |

## 0x53 - Rotate Head

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types#varint) | |
| Head yaw | [Byte](./data-types#byte) | |

## 0x63 - Set Entity Data

Push synced data fields. Field IDs and value encodings are documented separately — see [./entity-metadata](./entity-metadata).

## 0x65 - Set Entity Motion

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types#varint) | |
| Vx, Vy, Vz | [Short](./data-types#short) ×3 | `blocks/tick * 8000`. |

## 0x6B - Set Passengers

| Field | Type | Notes |
|-------|------|-------|
| Vehicle entity ID | [VarInt](./data-types#varint) | |
| Passenger count | [VarInt](./data-types#varint) | |
| Passenger IDs | [VarInt](./data-types#varint) × count | New passenger list (replaces, not appends). |

## 0x74 - Sound Entity

| Field | Type | Notes |
|-------|------|-------|
| Sound event | Sound holder | Either a registry ID (VarInt) or an inline sound event (see [./data-types](./data-types)). |
| Source category | [VarInt](./data-types#varint) | One of `master`, `music`, `record`, `weather`, `block`, `hostile`, `neutral`, `player`, `ambient`, `voice`. |
| Entity ID | [VarInt](./data-types#varint) | |
| Volume | [Float](./data-types#float) | |
| Pitch | [Float](./data-types#float) | |
| Seed | [Long](./data-types#long) | Determines the random subvariant chosen client-side. |

## 0x7C - Take Item Entity

| Field | Type | Notes |
|-------|------|-------|
| Collected entity ID | [VarInt](./data-types#varint) | The item / xp orb. |
| Collector entity ID | [VarInt](./data-types#varint) | Picker. |
| Pickup count | [VarInt](./data-types#varint) | Number of items picked up (for stack animation). |

## 0x7D - Teleport Entity

Carries an absolute position, rotation, and velocity, with a bitset of *relative* axes (when set, the corresponding component is treated as a delta to the client's existing value rather than a replacement — used for elytra/boat resync without snapping).

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types#varint) | |
| Position X / Y / Z | [Double](./data-types#double) | |
| Velocity X / Y / Z | [Double](./data-types#double) | |
| Yaw | [Float](./data-types#float) | Degrees. |
| Pitch | [Float](./data-types#float) | Degrees. |
| Relative flags | [Int](./data-types#int) (bitset) | Bits: 0=X, 1=Y, 2=Z, 3=Yaw, 4=Pitch, 5=VelX, 6=VelY, 7=VelZ, 8=rotate-velocity. |
| On ground | [Boolean](./data-types#boolean) | |

## 0x22 - Move Vehicle (serverbound)

Sent by the client to drive a player-controlled vehicle. See also [Move Vehicle (clientbound)](#0x39---move-vehicle-clientbound).

| Field | Type | Notes |
|-------|------|-------|
| Position X / Y / Z | [Double](./data-types#double) | |
| Yaw | [Float](./data-types#float) | |
| Pitch | [Float](./data-types#float) | |
| On ground | [Boolean](./data-types#boolean) | |

---

## Entity types and `Data`

The numeric `Type` field in [Add Entity](#0x01---add-entity) indexes into the `minecraft:entity_type` registry, whose IDs are part of the protocol contract for 26.1.2. The auxiliary `Data` integer is type-dependent; the most common uses are:

| Entity type | `Data` meaning |
|-------------|----------------|
| `falling_block` | Block-state numeric ID. |
| `item_frame`, `glow_item_frame`, `painting` | Hanging facing direction (0=down, 1=up, 2=north, 3=south, 4=west, 5=east). |
| `arrow`, `spectral_arrow`, `trident` | Shooter entity ID + 1 (0 = none). |
| `fishing_bobber` | Owner player entity ID. |
| `fireball`, `small_fireball`, `dragon_fireball`, `wither_skull` | Shooter entity ID + 1. |
| `experience_orb` | XP value. |
| All others | `0`. |

Most entity-specific state (variant, age, color, pose, nameplate, etc.) lives in the synced data fields delivered by [Set Entity Data](#0x63---set-entity-data); see [./entity-metadata](./entity-metadata).

> Source: net/minecraft/network/protocol/game/ClientboundAddEntityPacket.java, ClientboundMoveEntityPacket.java, ClientboundTeleportEntityPacket.java, ClientboundEntityPositionSyncPacket.java, ClientboundSetEntityDataPacket.java, ClientboundRemoveEntitiesPacket.java, ClientboundRotateHeadPacket.java, ClientboundSetEntityMotionPacket.java, ClientboundEntityEventPacket.java, ClientboundDamageEventPacket.java, ClientboundHurtAnimationPacket.java, ClientboundAnimatePacket.java, ClientboundTakeItemEntityPacket.java, ClientboundSetPassengersPacket.java, ClientboundMoveVehiclePacket.java, ClientboundSoundEntityPacket.java, ClientboundPlayerInfoUpdatePacket.java, ClientboundPlayerInfoRemovePacket.java, net/minecraft/world/entity/Entity.java, net/minecraft/world/entity/EntityType.java.
