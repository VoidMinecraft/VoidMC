# Text Component

A *Text Component* is the rich-text value transmitted in chat messages,
titles, item names, lore, sign text, written-book pages, scoreboard display
names, and many other places. In protocol 26.1.2 it is sent as
[NBT](./data-types#nbt) on the wire (see `ComponentSerialization.STREAM_CODEC`,
which delegates to the recursive `CODEC` against `NbtOps`).

The same format is also used in JSON form (`ComponentSerialization.CODEC`
parsed against `JsonOps`) when components appear in data files; see
[JSON Text Component](./data-types#json-text-component) for the legacy
String-wrapped wire form, still used in a handful of packets.

## Discriminated union

Top-level encoding is a three-way `Either`:

1. **Plain string** — interpreted as a literal `text` component. Used by the
   serialiser when the component has empty style and only a literal payload.
2. **Non-empty list of components** — interpreted as a literal-empty parent
   whose `extra` is the list. The first element supplies the parent's contents
   and style after appending.
3. **Compound** — a structured component as described below.

## Compound layout

A compound component is the concatenation of:

- exactly one **content discriminator** (selects the kind of contents),
- zero or more **content fields** for the chosen discriminator,
- the optional **style fields**, and
- the optional `extra` list of child components.

### Content discriminator

The discriminator is the presence of one of the keys below. If a `type` field
is present it is used directly; otherwise the contents are guessed by trying
each codec in turn.

| `type`        | Required key  | Meaning                                       |
|---------------|---------------|-----------------------------------------------|
| `text`        | `text`        | Literal string.                               |
| `translatable`| `translate`   | Localised string, see below.                  |
| `score`       | `score`       | Scoreboard value reference.                   |
| `selector`    | `selector`    | Entity-selector resolved on the receiver.     |
| `keybind`     | `keybind`     | Resolved as the user's binding for that key.  |
| `nbt`         | `nbt`         | NBT path read from a block / entity / storage. |
| `object`      | `object`      | Internal "object" content (e.g. atlas sprite).|

#### `text`
Fields:
- `text` — String.

#### `translatable`
Fields:
- `translate` — String. Translation key.
- `fallback` — Optional String. Used if the receiver has no entry for `translate`.
- `with` — Optional list. Each element is either a primitive (Number, Boolean,
  String) or a Text Component, substituted into the localised pattern at
  `%s` / `%1$s` / etc.

#### `score`
Fields under the `score` key:
- `name` — String, either an entity selector source or a literal scoreboard
  holder name.
- `objective` — String. Objective name.

#### `selector`
Fields:
- `selector` — String. Entity-selector source (e.g. `@a[distance=..10]`).
- `separator` — Optional Text Component, joined between resolved entities
  (default: a comma in white).

#### `keybind`
Fields:
- `keybind` — String. Internal key id (e.g. `key.jump`).

#### `nbt`
Fields:
- `nbt` — String. NBT path.
- `interpret` — Optional Boolean (default `false`). If true, each result is
  re-parsed as a Text Component.
- `plain` — Optional Boolean (default `false`). If true, results are emitted
  without snbt formatting. Mutually exclusive with `interpret`.
- `separator` — Optional Text Component.
- One of `block` (Position string), `entity` (selector string), or `storage`
  (Identifier) selects the data source.

#### `object`
Internal use; carries a sprite reference.

### Style fields

All style fields live at the root of the compound (and at the root of every
child in `extra`). All are optional. Absent means "inherit from parent."

| Field           | Type        | Notes                                              |
|-----------------|-------------|----------------------------------------------------|
| `color`         | String      | Either `#RRGGBB` or a named ChatFormatting colour. |
| `shadow_color`  | Int (ARGB)  | `0` disables the text shadow.                      |
| `bold`          | Boolean     |                                                    |
| `italic`        | Boolean     |                                                    |
| `underlined`    | Boolean     |                                                    |
| `strikethrough` | Boolean     |                                                    |
| `obfuscated`    | Boolean     |                                                    |
| `font`          | Identifier  | Font resource id; default `minecraft:default`.     |
| `insertion`     | String      | Inserted into chat input on shift-click.           |
| `click_event`   | Compound    | See [Click events](#click-events).                 |
| `hover_event`   | Compound    | See [Hover events](#hover-events).                 |

#### Click events

Compound with an `action` field selecting the variant. Each variant carries
exactly the fields its action requires:

| `action`              | Extra fields                                          |
|-----------------------|-------------------------------------------------------|
| `open_url`            | `url` (String, must parse as a URL)                   |
| `open_file`           | `path` (String) — only sent by clients to themselves  |
| `run_command`         | `command` (String, with leading `/`)                  |
| `suggest_command`     | `command` (String)                                    |
| `change_page`         | `page` (Int ≥ 1, written-book only)                   |
| `copy_to_clipboard`   | `value` (String)                                      |
| `show_dialog`         | `dialog` (Holder of `dialog`)                         |
| `custom`              | `id` (Identifier), `payload` (NBT)                    |

#### Hover events

Compound with an `action` field. Variants:

| `action`         | Extra fields                                                                |
|------------------|-----------------------------------------------------------------------------|
| `show_text`      | `value` (Text Component)                                                    |
| `show_item`      | `id` (item Identifier), `count` (Int, default 1), `components` (component patch) |
| `show_entity`    | `id` (entity-type Identifier), `uuid` (UUID String / int array), `name` (optional Text Component) |

### `extra`

Optional list of Text Components appended after the parent's own contents.
Children inherit any style field that they do not themselves set.

## Examples

A literal coloured string:

```json
{ "text": "Hello", "color": "gold", "bold": true }
```

A translatable with arguments:

```json
{
  "translate": "chat.type.text",
  "with": [
    { "text": "Steve" },
    "hi"
  ]
}
```

A list-form parent (collapsed by the serialiser when the head is a literal):

```json
[
  { "text": "Online: " },
  { "text": "0", "color": "green" }
]
```

A scoreboard reference rendered into chat:

```json
{
  "score": { "name": "@s", "objective": "deaths" },
  "color": "red"
}
```

## Size limits

The decoder applies the same `NbtAccounter` quotas as for any other NBT value
(see [NBT](./data-types#nbt)). Some packets impose tighter caps; for example
flat (collapsed) component strings used in chat may not exceed 262 144
characters (`flatRestrictedCodec`).
