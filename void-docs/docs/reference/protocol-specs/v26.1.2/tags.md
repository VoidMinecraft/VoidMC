# Tags

Tags are named groups of registry entries. They let data packs (and the network protocol) refer to "all logs" or "all leaves" or "all damage types caused by fire" without naming each entry individually. The client receives the server's view of every tag-group via the `Update Tags` packet so that gameplay behavior driven by tags (mining tool checks, fire-resistance lookups, recipe ingredients, biome categories, etc.) stays in lockstep with the server.

`Update Tags` is sent in the [Configuration state](./configuration) (just before `Finish Configuration`) and may be re-sent in the [Play state](./play-clientbound) when the server reloads.

## Wire layout

`Update Tags` is a single packet that carries a map of *tag-registries*. Each tag-registry is identified by the registry whose entries it groups (e.g. `minecraft:block`, `minecraft:item`, `minecraft:fluid`, `minecraft:entity_type`, `minecraft:damage_type`, `minecraft:worldgen/biome`).

| Field | Type | Notes |
|-------|------|-------|
| Tag-registry count | [VarInt](./data-types#varint) | Number of registries that follow. |
| Tag-registries | Array | Repeats `count` times. |

Each tag-registry entry:

| Field | Type | Notes |
|-------|------|-------|
| Registry ID | [Identifier](./data-types) | Which registry the following tags refer to. |
| Tag count | [VarInt](./data-types#varint) | Number of tags for this registry. |
| Tags | Array | Repeats `count` times. |

Each tag entry:

| Field | Type | Notes |
|-------|------|-------|
| Tag ID | [Identifier](./data-types) | Tag identifier (e.g. `minecraft:logs`). The leading `#` used in JSON is implicit. |
| Entry count | [VarInt](./data-types#varint) | Number of registry entries in the tag. |
| Entries | Array of [VarInt](./data-types#varint) | Numeric registry IDs of the entries. |

The numeric entry IDs are the same IDs the rest of the protocol uses for that registry (block state IDs use the block-state registry, etc.). Because the client is expected to know the full content of these registries (either built-in or reconstructed via [Registry Data](./registries) + Known Packs), each tag is purely a list of integer references — no resource identifiers per entry are sent.

```text
ClientboundUpdateTagsPacket
  VarInt   tag_registry_count
  for each tag_registry:
    Identifier registry_id
    VarInt   tag_count
    for each tag:
      Identifier tag_id
      VarInt[]   entry_ids   (length-prefixed VarInt array)
```

## Semantics

- A tag is **flat at the wire level**: nested tag references (`#minecraft:logs` inside `minecraft:planks` in JSON) are fully expanded by the server before transmission.
- The client treats each `Update Tags` packet as **authoritative and complete** for the registries it carries. Tags from a previous update for the same registry are discarded.
- Tags whose entries are unknown to the client (because of mismatched registry content) are dropped entry-by-entry; an empty resulting tag is still kept under its identifier.

## Tag-registries the client uses

The client expects (at minimum) tags for the following registries; missing entries fall back to client-side defaults but trigger desync warnings on tooltips/recipes:

- `minecraft:block` — mining tool tiers, mineable groups, fire-resistance lists.
- `minecraft:item` — recipe ingredient groups, equipment categories.
- `minecraft:fluid` — fluid behaviour groups (water/lava families).
- `minecraft:entity_type` — entity categorization (`#arrows`, `#raiders`).
- `minecraft:game_event` — vibration filtering for sculk sensors.
- `minecraft:damage_type` — damage immunities and effects.
- `minecraft:worldgen/biome` — biome categorization for music/spawning UI.
- `minecraft:enchantment` — enchantment grouping.
- `minecraft:instrument`, `minecraft:painting_variant`, `minecraft:cat_variant`, etc. — variant filtering.

Servers that strip a tag-registry entirely simply omit it from the packet.

> Source: net/minecraft/network/protocol/common/ClientboundUpdateTagsPacket.java, net/minecraft/tags/TagNetworkSerialization.java, net/minecraft/core/RegistrySynchronization.java.
