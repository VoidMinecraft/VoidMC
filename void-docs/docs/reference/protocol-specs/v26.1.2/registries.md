# Registries

Minecraft 26.1.2 transmits a number of *dynamic registries* from the server to the client during the [Configuration state](./configuration). These registries describe data-driven content that the client must know about (biomes, dimension types, chat formatting, damage types, mob variants, paintings, enchantments, instruments, jukebox songs, dialogs, etc.) and that the server may have customized via data packs.

This page specifies how the dynamic registry sync works on the wire, the bootstrap "Known Packs" handshake, the list of registries that participate, and the NBT shape of the most important entries.

## Mechanism

Dynamic registries are synchronized after Login and before Play, while the connection is in the Configuration state. The server sends one `Registry Data` packet per registry to be synchronized. Each packet carries:

| Field | Type | Notes |
|-------|------|-------|
| Registry ID | [Identifier](./data-types) | The registry being transmitted (e.g. `minecraft:worldgen/biome`). |
| Entries | Array of `([Identifier](./data-types#identifier), Optional [NBT](./data-types#nbt))` | Every entry the client should add to that registry. |

Each entry contains:

| Field | Type | Notes |
|-------|------|-------|
| Entry ID | [Identifier](./data-types) | The registry-key for the entry (e.g. `minecraft:plains`). |
| Has data | [Boolean](./data-types#boolean) | Whether the NBT payload follows. |
| Data | [NBT](./data-types) (network [NBT](./data-types#nbt), no name) | Only present when `Has data = true`. |

If `Has data` is false, the client uses its own built-in vanilla copy of that entry as supplied by the matching known pack. If `Has data` is true, the supplied NBT replaces (or defines) the entry.

The order of `Registry Data` packets is significant: they must be received before `Update Tags` (see [./tags](./tags)) for the same registries, and before `Finish Configuration`.

## Known Packs handshake

Before sending any `Registry Data`, the server sends `Select Known Packs` (clientbound). The client replies with `Select Known Packs` (serverbound) listing the data packs it already knows. A pack is identified by:

| Field | Type | Notes |
|-------|------|-------|
| Namespace | [String](./data-types#string) | Pack namespace; vanilla uses `"minecraft"`. |
| ID | [String](./data-types#string) | Pack identifier (e.g. `"core"`). |
| Version | [String](./data-types#string) | Pack version, typically the game version (e.g. `"1.21.10"`). |

For every entry in a registry that comes from a pack the client claims to have, the server is allowed to omit the NBT payload (`Has data = false`) and rely on the client to materialize the entry locally. In practice this means a vanilla client that reports the `minecraft:core` pack only receives NBT for *non-vanilla* entries, drastically reducing bandwidth at login.

If the client claims no packs, every entry must be sent with full NBT.

## Synchronized registries

The set of registries that must be sent (when their entries are not entirely covered by known packs) is:

- `minecraft:worldgen/biome`
- `minecraft:chat_type`
- `minecraft:trim_pattern`
- `minecraft:trim_material`
- `minecraft:wolf_variant`
- `minecraft:wolf_sound_variant`
- `minecraft:pig_variant`
- `minecraft:pig_sound_variant`
- `minecraft:frog_variant`
- `minecraft:cat_variant`
- `minecraft:cat_sound_variant`
- `minecraft:cow_variant`
- `minecraft:cow_sound_variant`
- `minecraft:chicken_variant`
- `minecraft:chicken_sound_variant`
- `minecraft:zombie_nautilus_variant`
- `minecraft:painting_variant`
- `minecraft:dimension_type`
- `minecraft:damage_type`
- `minecraft:banner_pattern`
- `minecraft:enchantment`
- `minecraft:jukebox_song`
- `minecraft:instrument`
- `minecraft:test_environment`
- `minecraft:test_instance`
- `minecraft:dialog`
- `minecraft:world_clock`
- `minecraft:timeline`

Other registries (blocks, items, entity types, fluids, particles, sound events, recipes, etc.) are not transmitted because their IDs are part of the protocol contract for a given version and are baked into the client.

## Entry NBT schemas

The NBT schema is the *network codec* of each registry's element (which can differ from its on-disk codec; some fields are stripped because they are server-only).

### `minecraft:worldgen/biome`

| Field | Type | Notes |
|-------|------|-------|
| `attributes` | [Compound](./data-types#compound) (optional) | Environment attribute overrides; defaults to empty. |
| `effects` | [Compound](./data-types#compound) | Visual/audio effects: `fog_color`, `water_color`, `water_fog_color`, `sky_color`, optional `foliage_color`, `grass_color`, optional `grass_color_modifier`, optional `particle`, optional `ambient_sound`, optional `mood_sound`, optional `additions_sound`, optional `music`. |

The non-network (data-pack) codec also has `has_precipitation`, `temperature`, `temperature_modifier`, `downfall`, `spawners`, `spawn_costs`, `carvers`, and `features`, but these are not sent over the wire.

### `minecraft:dimension_type`

| Field | Type | Notes |
|-------|------|-------|
| `has_fixed_time` | [Boolean](./data-types#boolean) (optional, default false) | If true, time is fixed at `fixed_time`. |
| `fixed_time` | [Long](./data-types#long) (optional) | Daytime tick when `has_fixed_time` is true. |
| `has_skylight` | [Boolean](./data-types#boolean) | Whether the dimension has skylight. |
| `has_ceiling` | [Boolean](./data-types#boolean) | Whether the dimension has a solid ceiling (Nether). |
| `has_ender_dragon_fight` | [Boolean](./data-types#boolean) | Whether to spawn the dragon fight. |
| `coordinate_scale` | [Double](./data-types#double) | Coordinate scaling factor (Nether = 8.0). |
| `min_y` | [Int](./data-types#int) | Minimum world Y. |
| `height` | [Int](./data-types#int) | Total Y range (multiple of 16). |
| `logical_height` | [Int](./data-types#int) | Maximum Y for portals/teleporters. |
| `infiniburn` | [Identifier](./data-types#identifier) | Tag (`#namespace:path`) of always-burning blocks. |
| `ambient_light` | [Float](./data-types#float) | Constant added to computed light. |
| `monster_settings` | [Compound](./data-types#compound) | `monster_spawn_light_level` (int provider) + `monster_spawn_block_light_limit` (int 0–15). |
| `skybox` | [Compound](./data-types#compound) (optional) | Sky rendering preset. |
| `cardinal_light` | [String](./data-types#string) enum (optional) | Cardinal light type. |
| `attributes` | [Compound](./data-types#compound) (optional) | Environment attribute overrides. |
| `timelines` | Holder set (optional) | List/tag of timeline references. |
| `default_clock` | [Compound](./data-types#compound) (optional) | World clock override. |

### `minecraft:chat_type`

| Field | Type | Notes |
|-------|------|-------|
| `chat` | [Compound](./data-types#compound) | Decoration applied to in-chat rendering. |
| `narration` | [Compound](./data-types#compound) | Decoration applied to narration. |

Each decoration has:

| Field | Type | Notes |
|-------|------|-------|
| `translation_key` | [String](./data-types#string) | Translation key, e.g. `"chat.type.text"`. |
| `parameters` | List of [String](./data-types#string) | Ordered list of parameter names (`"sender"`, `"target"`, `"content"`). |
| `style` | [Compound](./data-types#compound) (optional) | Text style applied to the rendered output. |

See [./chat](./chat) for how chat types are bound to messages.

### `minecraft:damage_type`

| Field | Type | Notes |
|-------|------|-------|
| `message_id` | [String](./data-types#string) | Translation key suffix used by death messages. |
| `scaling` | [String](./data-types#string) enum | `"never"`, `"when_caused_by_living_non_player"`, or `"always"`. |
| `exhaustion` | [Float](./data-types#float) | Hunger exhaustion induced. |
| `effects` | [String](./data-types#string) enum (optional) | Hurt sound class: `"hurt"`, `"thorns"`, `"drowning"`, `"burning"`, `"poking"`, `"freezing"`. Default `"hurt"`. |
| `death_message_type` | [String](./data-types#string) enum (optional) | `"default"`, `"fall_variants"`, `"intentional_game_design"`. |

### `minecraft:painting_variant`

| Field | Type | Notes |
|-------|------|-------|
| `asset_id` | [Identifier](./data-types#identifier) | Texture identifier. |
| `width` | [Int](./data-types#int) (1–16) | Painting width in blocks. |
| `height` | [Int](./data-types#int) (1–16) | Painting height in blocks. |
| `title` | Component (optional) | Display title. |
| `author` | Component (optional) | Display author. |

### `minecraft:wolf_variant` (and other mob variants)

The mob-variant registries (`wolf_variant`, `cat_variant`, `frog_variant`, `pig_variant`, `cow_variant`, `chicken_variant`, `zombie_nautilus_variant`) all follow the same shape over the wire — they reduce to a list of model assets:

| Field | Type | Notes |
|-------|------|-------|
| `assets` | [Compound](./data-types#compound) | Asset references for the model variant; structure is variant-specific (e.g. wolf has `wild`, `tame`, `angry`). |

The data-pack form additionally carries a `spawn_conditions` list and biome tag, but those are server-only and not in the network codec.

### `minecraft:enchantment`

| Field | Type | Notes |
|-------|------|-------|
| `description` | Component | Display name. |
| `exclusive_set` | Holder set (optional) | Items mutually exclusive with this enchantment. |
| `supported_items` | Holder set | Items that may carry this enchantment. |
| `primary_items` | Holder set (optional) | Items eligible from the table. |
| `weight` | [Int](./data-types#int) | Selection weight. |
| `max_level` | [Int](./data-types#int) | Maximum level. |
| `min_cost` / `max_cost` | [Compound](./data-types#compound) | Anvil/table cost ranges. |
| `anvil_cost` | [Int](./data-types#int) | Combine cost. |
| `slots` | List of [String](./data-types#string) | Equipment slots in which the enchantment is active. |
| `effects` | [Compound](./data-types#compound) | Component-style effect map. |

### `minecraft:jukebox_song`

| Field | Type | Notes |
|-------|------|-------|
| `sound_event` | Holder/Identifier | Sound to play; either a registry holder or inline `SoundEvent`. |
| `description` | Component | Display name shown in the jukebox tooltip. |
| `length_in_seconds` | [Float](./data-types#float) | Track length. |
| `comparator_output` | [Int](./data-types#int) (0–15) | Redstone signal strength. |

### `minecraft:instrument`

| Field | Type | Notes |
|-------|------|-------|
| `sound_event` | Holder/Identifier | Sound emitted when used. |
| `use_duration` | [Float](./data-types#float) | Use animation duration in seconds. |
| `range` | [Float](./data-types#float) | Audible range in blocks. |
| `description` | Component | Display name. |

### `minecraft:trim_pattern` / `minecraft:trim_material`

`trim_pattern`:

| Field | Type | Notes |
|-------|------|-------|
| `asset_id` | [Identifier](./data-types#identifier) | Texture identifier. |
| `description` | Component | Display name (used in tooltip). |
| `decal` | [Boolean](./data-types#boolean) | Whether the trim is rendered as a decal. |

`trim_material`:

| Field | Type | Notes |
|-------|------|-------|
| `assets` | [Compound](./data-types#compound) | Per-armor-material override map. |
| `description` | Component | Tooltip display component. |

### `minecraft:banner_pattern`

| Field | Type | Notes |
|-------|------|-------|
| `asset_id` | [Identifier](./data-types#identifier) | Texture identifier. |
| `translation_key` | [String](./data-types#string) | Translation key for the tooltip. |

### `minecraft:dialog`

A self-contained UI dialog definition (introduced in 1.21.6+). The schema is large and deeply nested (buttons, body items, action lists). Treat the payload as an opaque NBT compound on the wire and decode it with the dialog codec.

### `minecraft:test_environment` and `minecraft:test_instance`

Game-test definitions used by the `/test` command. Sent for parity with the integrated server and may be empty in production deployments.

> NOTE: For registries whose network codec equals their data codec (`DIRECT_CODEC`), the on-disk JSON shape under `data/<namespace>/<registry path>/` translates field-for-field into the wire NBT.

> Source: net/minecraft/resources/RegistryDataLoader.java, net/minecraft/core/RegistrySynchronization.java, net/minecraft/core/registries/Registries.java, net/minecraft/network/protocol/configuration/ClientboundRegistryDataPacket.java, net/minecraft/network/protocol/configuration/ClientboundSelectKnownPacks.java, net/minecraft/network/protocol/configuration/ServerboundSelectKnownPacks.java, net/minecraft/server/packs/repository/KnownPack.java, net/minecraft/world/level/biome/Biome.java, net/minecraft/world/level/dimension/DimensionType.java, net/minecraft/network/chat/ChatType.java, net/minecraft/world/damagesource/DamageType.java, net/minecraft/world/entity/decoration/PaintingVariant.java, net/minecraft/world/item/enchantment/Enchantment.java, net/minecraft/world/item/JukeboxSong.java, net/minecraft/world/item/Instrument.java.
