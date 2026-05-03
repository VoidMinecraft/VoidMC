# Slot / ItemStack

A *slot* is the wire encoding of a single `ItemStack` plus its overlay of data
components. The canonical codec is
`ItemStack.OPTIONAL_STREAM_CODEC` (which the non-optional `STREAM_CODEC`
delegates to and additionally rejects empty stacks).

## Wire layout

```text
+-------------------+----------------------+--------------------------+
| Item Count        | Item ID  (only if    | Components Patch         |
| (VarInt)          | Count > 0; VarInt)   | (only if Count > 0)      |
+-------------------+----------------------+--------------------------+
```

- **Item Count** — VarInt. `0` (or any negative value, when accepted) means
  the slot is empty (`ItemStack.EMPTY`); the rest of the slot is then absent.
  Otherwise the value is the stack size; the encoder rejects sizes greater
  than the item's `max_stack_size` component.
- **Item ID** — VarInt id within the `minecraft:item` registry, written via
  `Item.STREAM_CODEC`. Also accepts a `Holder<Item>` direct value (id `0`
  followed by the item payload), but vanilla servers and clients only send
  registry references in this position.
- **Components Patch** — see [Data Component patch](#data-component-patch).

The non-optional `STREAM_CODEC` is identical but throws on an empty stack
(used in places where an empty value would be invalid, e.g. inside crafting
recipes).

The "untrusted" variant `OPTIONAL_UNTRUSTED_STREAM_CODEC` wraps each
individual component payload in a length prefix, so that a malformed component
type cannot desync the rest of the buffer; it is used for stacks coming from
the client.

## Data Component patch

A *patch* is a partial mutation against an item type's *prototype* component
map: it can add or replace entries, and it can mark prototype entries for
removal.

```text
+------------------------+------------------------+
| Added Count   (VarInt) | Removed Count (VarInt) |
+------------------------+------------------------+
| repeated Added Count times:                     |
|   Component Type ID (VarInt, see below)         |
|   Component Payload (codec depends on type)     |
+-------------------------------------------------+
| repeated Removed Count times:                   |
|   Component Type ID (VarInt)                    |
+-------------------------------------------------+
```

If both counts are zero the patch is empty and the stack carries the item
type's prototype components unmodified.

**Component Type ID** is a VarInt id within the `minecraft:data_component_type`
registry. The id space is fixed by the protocol version; a client and server
on the same version share the table. The payload's encoding is determined
entirely by the type — there is no length prefix in the trusted form.

In the *untrusted* variant (`OPTIONAL_UNTRUSTED_STREAM_CODEC`), each component
payload is preceded by a VarInt byte length and decoded against a sliced
buffer.

### Removed entries

Each entry in the *Removed* section names a component the receiver should
delete from its prototype map for this stack. It is an error to mark a
component as both added and removed in the same patch.

### Common component types

The full list lives in `net.minecraft.core.component.DataComponents`. The
following are the most frequently observed on the wire. Each row gives the
registry id (the `minecraft:` namespace is implied) and the payload encoding.

| Identifier              | Payload                                                            |
|-------------------------|--------------------------------------------------------------------|
| `custom_data`           | NBT Compound. Free-form server/data-pack data; not interpreted.    |
| `max_stack_size`        | VarInt (1–99).                                                     |
| `max_damage`            | VarInt (≥ 1).                                                      |
| `damage`                | VarInt (≥ 0).                                                      |
| `unbreakable`           | Empty (`Unit`).                                                    |
| `custom_name`           | [Text Component](./text-component).                                |
| `item_name`             | [Text Component](./text-component) (overrides default name).       |
| `item_model`            | [Identifier](./data-types#identifier).                             |
| `lore`                  | Prefixed array of [Text Component](./text-component).              |
| `rarity`                | VarInt enum (0=common, 1=uncommon, 2=rare, 3=epic).                |
| `enchantments`          | Map of (enchantment Holder → VarInt level) + Boolean `show_in_tooltip` (legacy). |
| `can_place_on`          | Adventure-mode block predicate list.                               |
| `can_break`             | Adventure-mode block predicate list.                               |
| `attribute_modifiers`   | Prefixed array of attribute-modifier entries + display flag.       |
| `custom_model_data`     | Prefixed array of floats / flags / strings / colours.              |
| `tooltip_display`       | Boolean `hide_tooltip` + BitSet of hidden component types.         |
| `repair_cost`           | VarInt.                                                            |
| `creative_slot_lock`    | Empty.                                                             |
| `enchantment_glint_override` | Boolean.                                                      |
| `food`                  | VarInt nutrition + Float saturation + Boolean `can_always_eat`.    |
| `consumable`            | Float consume seconds + animation enum + sound + particles + effects. |
| `use_remainder`         | A nested ItemStack.                                                |
| `use_cooldown`          | Float seconds + Optional Identifier group.                         |
| `damage_resistant`      | Identifier (damage-type tag).                                      |
| `tool`                  | Rules list + default mining speed + damage per block.              |
| `weapon`                | Item damage per attack + disable-blocking-for ticks.               |
| `enchantable`           | VarInt enchantment value.                                          |
| `equippable`            | Equipment slot + sound + asset id + camera overlay + allowed entities. |
| `repairable`            | HolderSet of allowed repair items.                                 |
| `glider`                | Empty.                                                             |
| `tooltip_style`         | Identifier.                                                        |
| `death_protection`      | Effects to apply on near-death save.                               |
| `dyed_color`            | Int (RGB).                                                         |
| `map_id`                | VarInt.                                                            |
| `map_decorations`       | NBT Compound describing markers.                                   |
| `charged_projectiles`   | Prefixed array of ItemStack.                                       |
| `bundle_contents`       | Prefixed array of ItemStack.                                       |
| `potion_contents`       | Optional potion Holder + Optional Int colour + custom effects.     |
| `container`             | Prefixed array of slot/ItemStack pairs.                            |
| `stored_enchantments`   | Same shape as `enchantments`; stored in books.                     |
| `block_state`           | Map of String → String (block-state property overrides).           |
| `bees`                  | Prefixed array of bee entries (NBT + ticks-in-hive + min ticks).   |
| `lock`                  | NBT predicate.                                                     |
| `note_block_sound`      | Identifier.                                                        |
| `instrument`            | Holder of `instrument`.                                            |
| `recipes`               | Prefixed array of Identifier (unlocked recipes for knowledge book). |
| `firework_explosion`    | Shape enum + colours + fade colours + trail + flicker.             |
| `fireworks`             | VarInt flight duration + array of explosions.                      |
| `profile`               | Resolved game profile (for player heads).                          |
| `banner_patterns`       | Array of (pattern Holder + DyeColor enum).                         |
| `base_color`            | DyeColor enum.                                                     |
| `pot_decorations`       | Four item-Holder ids (sherds).                                     |
| `writable_book_content` | Array of String pages (raw author text).                           |
| `written_book_content`  | Array of `Filterable<Text Component>` pages + title + author + generation. |
| `trim`                  | Material Holder + pattern Holder.                                  |
| `debug_stick_state`     | Map of block Holder → String (last selected property per block).   |
| `bucket_entity_data`    | NBT Compound.                                                      |
| `block_entity_data`     | NBT Compound (synced into the bound block entity on placement).    |

For the exact codec of each component see the corresponding class under
`net.minecraft.world.item.component.*` or the inline registration in
`DataComponents.java`. Components flagged `transient()` in `DataComponents`
are server-only and never appear in a patch on the wire.

> NOTE: The `data_component_type` registry order is part of the protocol
> contract. Servers and clients of the same protocol version (26.1.2) share
> a fixed numeric id for each component identifier; ids are not negotiated.

## Empty slot

An empty slot is encoded as a single VarInt `0` and nothing else. Item ID and
the component patch are both omitted.
