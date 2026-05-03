# Entity Metadata

The `Set Entity Data` packet (clientbound; see [./entities](./entities)) carries an entity's *synced data fields* — a sparse map of typed values keyed by an unsigned-byte index. Every entity class declares its set of fields with a stable index assignment that subclasses extend.

## Wire layout

`Set Entity Data` is:

```text
VarInt entity_id
loop:
  UByte index
  if index == 0xFF: break
  VarInt serializer_type_id
  <value>            (length and shape determined by serializer_type_id)
```

The `0xFF` byte is the *terminator* marker. A packet with no fields therefore has body `[VarInt entity_id, 0xFF]`.

`serializer_type_id` is an index into the global `EntityDataSerializers` registry (a numeric ID, registered in fixed order by the client; see the table below).

## Serializer types

The order below matches the registration order in vanilla and therefore the wire IDs.

| ID | Name | Encoding |
|----|------|----------|
| 0 | [Byte](./data-types#byte) | 1 byte. |
| 1 | [VarInt](./data-types#varint) | VarInt. |
| 2 | [Long](./data-types#long) | VarLong. |
| 3 | [Float](./data-types#float) | 4 bytes IEEE 754. |
| 4 | [String](./data-types#string) | VarInt length + UTF-8 bytes. |
| 5 | Component | "Trusted" [text component](./text-component) (network NBT). |
| 6 | Optional Component | Boolean present; if true, Component. |
| 7 | [Slot](./slot) / [ItemStack](./slot) | See [./slot](./slot). |
| 8 | [Boolean](./data-types#boolean) | 1 byte (0 / 1). |
| 9 | Rotations | 3 × Float (pitch, yaw, roll, in degrees). |
| 10 | BlockPos | Long (packed 26-bit X / 12-bit Y / 26-bit Z). |
| 11 | Optional BlockPos | Boolean present; if true, BlockPos. |
| 12 | Direction | VarInt enum (0=down, 1=up, 2=north, 3=south, 4=west, 5=east). |
| 13 | Optional living entity reference | Boolean present; if true, UUID of the referenced living entity. |
| 14 | Block State | VarInt — block-state registry ID. |
| 15 | Optional Block State | VarInt; `0` means absent, otherwise `id` (encoded as `id + 0`; vanilla uses `0` as sentinel for "no state"). |
| 16 | [Particle](./particle) | Particle ID (VarInt) + type-specific options (see [./particle](./particle)). |
| 17 | Particles | VarInt count + repeated Particle. |
| 18 | Villager Data | VarInt type + VarInt profession + VarInt level. |
| 19 | Optional Unsigned [Int](./data-types#int) | VarInt; `0` means absent, otherwise `value + 1`. |
| 20 | Pose | VarInt enum (0=standing, 1=fall_flying, 2=sleeping, 3=swimming, 4=spin_attack, 5=crouching, 6=long_jumping, 7=dying, 8=croaking, 9=using_tongue, 10=sitting, 11=roaring, 12=sniffing, 13=emerging, 14=digging, 15=sliding, 16=shooting, 17=inhaling — exact values follow the `Pose` enum order). |
| 21 | Cat Variant | VarInt — registry holder ID into `minecraft:cat_variant`. |
| 22 | Cat Sound Variant | VarInt — registry holder ID. |
| 23 | Cow Variant | VarInt — registry holder ID. |
| 24 | Cow Sound Variant | VarInt — registry holder ID. |
| 25 | Wolf Variant | VarInt — registry holder ID. |
| 26 | Wolf Sound Variant | VarInt — registry holder ID. |
| 27 | Frog Variant | VarInt — registry holder ID. |
| 28 | Pig Variant | VarInt — registry holder ID. |
| 29 | Pig Sound Variant | VarInt — registry holder ID. |
| 30 | Chicken Variant | VarInt — registry holder ID. |
| 31 | Chicken Sound Variant | VarInt — registry holder ID. |
| 32 | Zombie Nautilus Variant | VarInt — registry holder ID. |
| 33 | Optional Global [Position](./data-types#position) | Boolean present; if true, dimension Identifier + BlockPos. |
| 34 | Painting Variant | VarInt — registry holder ID into `minecraft:painting_variant`. |
| 35 | Sniffer State | VarInt enum (0=idling, 1=feeling_happy, 2=scenting, 3=sniffing, 4=searching, 5=digging, 6=rising). |
| 36 | Armadillo State | VarInt enum (0=idle, 1=rolling, 2=scared, 3=unrolling). |
| 37 | Copper Golem State | VarInt enum. |
| 38 | Weathering Copper State | VarInt enum (0=unaffected, 1=exposed, 2=weathered, 3=oxidized). |
| 39 | Vector3 | 3 × Float. |
| 40 | Quaternion | 4 × Float (x, y, z, w). |
| 41 | Resolvable Profile | Optional name + UUID + property set (see profile encoding). |
| 42 | Humanoid Arm | VarInt enum (0=left, 1=right). |

> NOTE: A serializer type (and its wire ID) can shift between vanilla snapshots when new types are added. The IDs above match the registration order in 26.1.2's `EntityDataSerializers`.

The "registry holder" types (variants, sound variants, painting variants) use the standard holder encoding: a positive VarInt is `id + 1` referring to the dynamic registry entry, and `0` means an inline payload follows (rare for variants; primarily used for sound events).

## Indexed schema by class

The `index` byte assignment is hierarchical. Each subclass *appends* indices starting after its parent's last assigned index. The full chain for a typical mob is `Entity → LivingEntity → Mob → AgeableMob → Animal → <species>`.

### `Entity` (base — indices 0..7)

| Index | Type | Field |
|-------|------|-------|
| 0 | [Byte](./data-types#byte) (flags) | Bitset: 0x01 on fire, 0x02 sneaking, 0x04 unused, 0x08 sprinting, 0x10 swimming, 0x20 invisible, 0x40 glowing, 0x80 elytra-flying. |
| 1 | [VarInt](./data-types#varint) | Air ticks (default 300). |
| 2 | Optional Component | Custom name. |
| 3 | [Boolean](./data-types#boolean) | Custom name visible. |
| 4 | [Boolean](./data-types#boolean) | Silent. |
| 5 | [Boolean](./data-types#boolean) | No gravity. |
| 6 | Pose | Current pose. |
| 7 | [VarInt](./data-types#varint) | Frozen ticks (powder snow). |

### `LivingEntity` (extends Entity — indices 8..14)

| Index | Type | Field |
|-------|------|-------|
| 8 | [Byte](./data-types#byte) (flags) | 0x01 hand active, 0x02 offhand, 0x04 in spin attack. |
| 9 | [Float](./data-types#float) | Health. |
| 10 | [VarInt](./data-types#varint) | Potion effect particle color (packed ARGB or 0 if none). |
| 11 | [Boolean](./data-types#boolean) | Potion effect particles ambient. |
| 12 | [VarInt](./data-types#varint) | Number of arrows stuck. |
| 13 | [VarInt](./data-types#varint) | Number of bee stingers. |
| 14 | Optional BlockPos | Sleeping position. |

### `Mob` (extends LivingEntity — index 15)

| Index | Type | Field |
|-------|------|-------|
| 15 | [Byte](./data-types#byte) (flags) | 0x01 no AI, 0x02 left-handed, 0x04 aggressive. |

### `Player` (extends LivingEntity — indices 15..20)

| Index | Type | Field |
|-------|------|-------|
| 15 | [Float](./data-types#float) | Additional hearts (absorption). |
| 16 | [VarInt](./data-types#varint) | Score (used in death screen). |
| 17 | [Byte](./data-types#byte) | Skin part bitmask: 0x01 cape, 0x02 jacket, 0x04 left sleeve, 0x08 right sleeve, 0x10 left pants, 0x20 right pants, 0x40 hat. |
| 18 | [Byte](./data-types#byte) | Main hand (0=left, 1=right). |
| 19 | [NBT](./data-types#nbt) compound | Left shoulder entity (parrot). |
| 20 | [NBT](./data-types#nbt) compound | Right shoulder entity (parrot). |

### Common species extensions

Each entity class defines its own additional indices on top of `Mob`. For example:

- `Wolf` adds: tame flags byte, owner UUID (optional), collar color VarInt, variant holder, sound variant holder.
- `Cat` adds: variant holder, sound variant holder, lying-down boolean, relaxed-meow boolean, collar color.
- `Painting` (extends Entity, not Mob): variant holder at the next free index after Entity.
- `Display` family (`item_display`, `block_display`, `text_display`): a large block of transformation fields (interpolation start, duration, scale, translation, rotation, billboard mode, brightness, view range, shadow radius, shadow strength, width, height, glow color override, plus type-specific fields).

The authoritative per-class index tables live in each subclass's static initializer in the vanilla source (see the `defineId(...)` calls in classes under `net.minecraft.world.entity.*`).

> Source: net/minecraft/network/syncher/EntityDataSerializers.java, net/minecraft/network/syncher/SynchedEntityData.java, net/minecraft/network/protocol/game/ClientboundSetEntityDataPacket.java, net/minecraft/world/entity/Entity.java, net/minecraft/world/entity/LivingEntity.java, net/minecraft/world/entity/Mob.java, net/minecraft/world/entity/player/Player.java, net/minecraft/world/entity/Pose.java.
