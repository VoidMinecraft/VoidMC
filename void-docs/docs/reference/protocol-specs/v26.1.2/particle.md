# Particle

A *particle* descriptor is a `ParticleOptions` value: a particle type plus,
for some types, a small payload of type-specific options. Particles appear in
`ClientboundLevelParticlesPacket`, `ClientboundExplodePacket`, and inside a
few [Slot](./slot) data components (e.g. `consumable`).

## Wire layout

```text
+-------------------+--------------------------+
| Particle Type ID  | Type-specific Payload    |
| (VarInt)          | (codec depends on type)  |
+-------------------+--------------------------+
```

- **Particle Type ID** — VarInt id within the `minecraft:particle_type`
  registry. The mapping from id to identifier is part of the protocol
  contract for version 26.1.2.
- **Payload** — for `SimpleParticleType` entries, empty (zero bytes). For
  parameterised types, the bytes described in [Parameterised types](#parameterised-types)
  below.

## SimpleParticleType

Most particle types are *simple*: they carry no payload, and the wire form is
just the registry id. Examples include `flame`, `cloud`, `crit`, `smoke`,
`portal`, `note`, `heart`, `lava`, `splash`, `bubble`, `enchant`,
`firework`, `dragon_breath` (no, that one is parameterised — see below),
`damage_indicator`, `explosion`, `explosion_emitter`, `electric_spark`,
`scrape`, `wax_on`, `wax_off`, `glow`, `glow_squid_ink`, `squid_ink`,
`current_down`, `bubble_column_up`, `bubble_pop`, `nautilus`, `dolphin`,
`firefly`, `cherry_leaves`, `pale_oak_leaves`, `dust_plume`,
`sculk_soul`, `sculk_charge_pop`, `soul`, `soul_fire_flame`,
`small_gust`, `gust`, `gust_emitter_small`, `gust_emitter_large`,
`sonic_boom`, `trial_spawner_detection`, `trial_spawner_detection_ominous`,
`vault_connection`, `ominous_spawning`, `raid_omen`, `trial_omen`,
`infested`, `egg_crack`, …

The complete list lives in `net.minecraft.core.particles.ParticleTypes`.

## Parameterised types

The following particle types carry an extra payload after the registry id.
Codec sources are under `net.minecraft.core.particles.*Options`.

### `block`, `block_marker`, `falling_dust`, `dust_pillar`, `block_crumble`

Carry a single block state as a VarInt id within the global block-state
registry (`Block.BLOCK_STATE_REGISTRY`).

```text
+-------------------------+
| Block State ID (VarInt) |
+-------------------------+
```

### `dust`

```text
+----------------+---------------+
| Color (Int)    | Scale (Float) |
+----------------+---------------+
```

`Color` is packed as `0x00RRGGBB`. `Scale` is clamped client-side to
`[0.01, 4.0]`.

### `dust_color_transition`

```text
+--------------------+------------------+---------------+
| From Color (Int)   | To Color (Int)   | Scale (Float) |
+--------------------+------------------+---------------+
```

### `entity_effect`, `tinted_leaves`, `flash`

A single `int` ARGB colour (`0xAARRGGBB`).

```text
+-------------+
| Color (Int) |
+-------------+
```

### `effect`, `instant_effect`

```text
+-------------+----------------+
| Color (Int) | Power (Float)  |
+-------------+----------------+
```

`Color = -1` means "use the default colour for the effect."

### `dragon_breath`

```text
+----------------+
| Power (Float)  |
+----------------+
```

### `item`

```text
+--------------------------+
| ItemStackTemplate        |
+--------------------------+
```

`ItemStackTemplate` is a thin variant of [Slot](./slot): a non-empty stack
without a stack count (Item Holder + components patch).

### `vibration`

```text
+-----------------------+--------------------------+
| Position Source       | Arrival Ticks (VarInt)   |
+-----------------------+--------------------------+
```

`Position Source` is a tagged union (VarInt source-type id within the
`minecraft:position_source_type` registry, then source-specific payload):

- `block`: a [Position](./data-types#position).
- `entity`: a VarInt entity id and a Float eye-offset Y.

(The `entity` source is rejected by the particle's payload validator and so
should not appear in vibration particles in practice.)

### `shriek`

```text
+----------------+
| Delay (VarInt) |
+----------------+
```

Number of ticks before the shriek is rendered.

### `sculk_charge`

```text
+----------------+
| Roll (Float)   |
+----------------+
```

The render rotation in radians.

### `trail`

```text
+------------------------+----------------+-------------------+
| Target  (Vec3, 3×Double)| Color (Int)   | Duration (VarInt) |
+------------------------+----------------+-------------------+
```

The trail is drawn from its emit position toward `Target` over `Duration`
ticks.

## Notes

- All `Float`s are IEEE-754 single-precision big-endian; `Vec3` is three
  big-endian `Double`s.
- The id used for the **Particle Type** field is the **particle-type registry
  id**, not the registry id of the option payload. There is one fixed table
  per protocol version.
- Adding a new particle type at the protocol level always changes the id of
  every particle declared after it; clients and servers must agree on the
  protocol version before sending particles.
