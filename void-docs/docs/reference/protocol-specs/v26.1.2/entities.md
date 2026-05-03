# Entities

This page specifies the wire format used to manage entities in the [Play state](./play-clientbound). Per-entity *metadata* (the synced data fields) lives in its own page, see [./entity-metadata](./entity-metadata).

The server is fully authoritative over entity state. Clients receive a stream of small mutation packets that reference entities by their server-assigned **Entity ID** (a VarInt; 0 is reserved for "no entity" in some packets). UUIDs are only sent at spawn time and for player-info bookkeeping.

## Lifecycle

| Packet | Direction | Purpose |
|--------|-----------|---------|
| `Add Entity` | Clientbound | Spawn an entity (any type, including the player's own object form when seen by others). |
| `Remove Entities` | Clientbound | Despawn one or more entities by ID. |
| `Set Entity Data` | Clientbound | Push synced data fields (see [./entity-metadata](./entity-metadata)). |
| `Update Entity Position` / `Position+Rotation` / `Rotation` | Clientbound | Small relative movement (delta encoded). |
| `Teleport Entity` | Clientbound | Absolute reposition with optional relative-flags. |
| `Entity Position Sync` | Clientbound | Authoritative resync of position+velocity (no animation). |
| `Set Head Rotation` | Clientbound | Updates only the head yaw. |
| `Entity Velocity` | Clientbound | Sets velocity for client-side prediction. |
| `Entity Event` | Clientbound | Status code (one byte) — death animations, totem of undying, etc. |
| `Damage Event` | Clientbound | Modern signed damage notification (replaces legacy `Animate(hurt)`). |
| `Hurt Animation` | Clientbound | Pure visual hurt direction. |
| `Animate` | Clientbound | One of: swing, leave bed, critical effect, magic critical, swing offhand. |
| `Take Item Entity` | Clientbound | Animates an item being collected by an entity. |
| `Set Passengers` | Clientbound | Replaces the passenger list of a vehicle. |
| `Move Vehicle` | Serverbound *and* Clientbound | Player-controlled vehicle movement. |
| `Sound Entity` | Clientbound | Plays a sound attached to an entity. |
| `Player Info Update` / `Player Info Remove` | Clientbound | Manages tablist + per-player chat session keys. |

## Add Entity

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

The `Data` field semantics depend on the entity type. For most entity types it is `0`. Notable users include `falling_block` (block state ID), `item_display` / `block_display` (display identifier), arrows and other projectiles (shooter entity ID + 1; 0 means "no shooter").

## Movement packets

Position updates use one of three packet shapes depending on whether they include rotation, position, or both. Small movements use **delta encoding**:

| Field | Type | Notes |
|-------|------|-------|
| Δ position (x/y/z) | [Short](./data-types#short) each | Encodes `current - previous` in 1/4096 of a block (`short = (Δblocks * 4096)`). |

A delta packet is only valid while the entity has not moved more than ±8 blocks since the last absolute teleport (because `±8 blocks * 4096 = ±32768` saturates the short). If a movement exceeds that range, or to defeat accumulated rounding error, the server sends `Teleport Entity` instead.

`Update Entity Position`:

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types#varint) | |
| Δx, Δy, Δz | [Short](./data-types#short) ×3 | Delta-encoded. |
| On ground | [Boolean](./data-types#boolean) | |

`Update Entity Position and Rotation` adds:

| Field | Type | Notes |
|-------|------|-------|
| Yaw | [Byte](./data-types#byte) | 1/256ths of a turn. |
| Pitch | [Byte](./data-types#byte) | 1/256ths of a turn. |
| On ground | [Boolean](./data-types#boolean) | |

`Update Entity Rotation`:

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types#varint) | |
| Yaw | [Byte](./data-types#byte) | |
| Pitch | [Byte](./data-types#byte) | |
| On ground | [Boolean](./data-types#boolean) | |

`Set Head Rotation`:

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types#varint) | |
| Head yaw | [Byte](./data-types#byte) | |

## Teleport Entity

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

## Entity Position Sync

Plain authoritative override (no smoothing on the client). Same field layout as `Teleport Entity` minus the relative flags.

## Entity Velocity

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types#varint) | |
| Vx, Vy, Vz | [Short](./data-types#short) ×3 | `blocks/tick * 8000`. |

## Entity Event

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [Int](./data-types#int) | (32-bit Int — not VarInt; legacy wire shape.) |
| Event ID | [Byte](./data-types#byte) | Status code. See `EntityEvent` for the full enumeration (e.g. 2 = hurt, 3 = death, 35 = totem, 60 = explode). |

## Damage Event

Signed/typed damage notification used for hurt animation + sound + camera direction.

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types#varint) | Damaged entity. |
| Source type | [VarInt](./data-types#varint) | ID into the `minecraft:damage_type` registry. |
| Source cause ID | [VarInt](./data-types#varint) | Direct cause entity ID + 1; 0 = none. |
| Source direct ID | [VarInt](./data-types#varint) | Indirect cause entity ID + 1; 0 = none. |
| Has source position | [Boolean](./data-types#boolean) | |
| Source X / Y / Z | [Double](./data-types#double) | Only when `Has source position` is true. |

## Hurt Animation

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types#varint) | |
| Yaw | [Float](./data-types#float) | Direction (degrees) the camera/model should flinch from. |

## Animate

| Field | Type | Notes |
|-------|------|-------|
| Entity ID | [VarInt](./data-types#varint) | |
| Animation | UByte | 0 = swing main hand, 2 = leave bed, 3 = swing off hand, 4 = critical effect, 5 = magic critical effect. |

## Take Item Entity

| Field | Type | Notes |
|-------|------|-------|
| Collected entity ID | [VarInt](./data-types#varint) | The item / xp orb. |
| Collector entity ID | [VarInt](./data-types#varint) | Picker. |
| Pickup count | [VarInt](./data-types#varint) | Number of items picked up (for stack animation). |

## Set Passengers

| Field | Type | Notes |
|-------|------|-------|
| Vehicle entity ID | [VarInt](./data-types#varint) | |
| Passenger count | [VarInt](./data-types#varint) | |
| Passenger IDs | [VarInt](./data-types#varint) × count | New passenger list (replaces, not appends). |

## Move Vehicle

Sent in both directions to keep player-controlled vehicles in sync.

| Field | Type | Notes |
|-------|------|-------|
| Position X / Y / Z | [Double](./data-types#double) | |
| Yaw | [Float](./data-types#float) | |
| Pitch | [Float](./data-types#float) | |
| On ground | [Boolean](./data-types#boolean) | (Serverbound only.) |

## Sound Entity

| Field | Type | Notes |
|-------|------|-------|
| Sound event | Sound holder | Either a registry ID (VarInt) or an inline sound event (see [./data-types](./data-types)). |
| Source category | [VarInt](./data-types#varint) | One of `master`, `music`, `record`, `weather`, `block`, `hostile`, `neutral`, `player`, `ambient`, `voice`. |
| Entity ID | [VarInt](./data-types#varint) | |
| Volume | [Float](./data-types#float) | |
| Pitch | [Float](./data-types#float) | |
| Seed | [Long](./data-types#long) | Determines the random subvariant chosen client-side. |

## Player Info Update / Remove

A player is also an entity, but tablist/chat-session bookkeeping is independent of the entity ID. `Player Info Update` carries an action bitset (add player, initialize chat, update gamemode, update listed, update latency, update display name, update list priority, update show-hat) plus per-action payloads keyed by player UUID. `Player Info Remove` is just a list of UUIDs to drop.

These packets are how the client learns of a player's chat session public key (see [./chat](./chat)) and gamemode changes that occur on other clients.

## Entity types and `Data`

The numeric `Type` field in `Add Entity` indexes into the `minecraft:entity_type` registry, whose IDs are part of the protocol contract for 26.1.2. The auxiliary `Data` integer is type-dependent; the most common uses are:

| Entity type | `Data` meaning |
|-------------|----------------|
| `falling_block` | Block-state numeric ID. |
| `item_frame`, `glow_item_frame`, `painting` | Hanging facing direction (0=down, 1=up, 2=north, 3=south, 4=west, 5=east). |
| `arrow`, `spectral_arrow`, `trident` | Shooter entity ID + 1 (0 = none). |
| `fishing_bobber` | Owner player entity ID. |
| `fireball`, `small_fireball`, `dragon_fireball`, `wither_skull` | Shooter entity ID + 1. |
| `experience_orb` | XP value. |
| All others | `0`. |

Most entity-specific state (variant, age, color, pose, nameplate, etc.) lives in the synced data fields delivered by `Set Entity Data`; see [./entity-metadata](./entity-metadata).

> Source: net/minecraft/network/protocol/game/ClientboundAddEntityPacket.java, ClientboundMoveEntityPacket.java, ClientboundTeleportEntityPacket.java, ClientboundEntityPositionSyncPacket.java, ClientboundSetEntityDataPacket.java, ClientboundRemoveEntitiesPacket.java, ClientboundRotateHeadPacket.java, ClientboundSetEntityMotionPacket.java, ClientboundEntityEventPacket.java, ClientboundDamageEventPacket.java, ClientboundHurtAnimationPacket.java, ClientboundAnimatePacket.java, ClientboundTakeItemEntityPacket.java, ClientboundSetPassengersPacket.java, ClientboundMoveVehiclePacket.java, ClientboundSoundEntityPacket.java, ClientboundPlayerInfoUpdatePacket.java, ClientboundPlayerInfoRemovePacket.java, net/minecraft/world/entity/Entity.java, net/minecraft/world/entity/EntityType.java.
