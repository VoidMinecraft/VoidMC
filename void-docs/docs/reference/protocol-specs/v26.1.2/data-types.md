# Data Types

This page specifies every primitive wire encoding used by the Minecraft Java
Edition protocol, version 26.1.2. All multi-byte integers and floats are
**big-endian**. Unless stated otherwise, signed integers use two's complement.

The canonical implementations are `net.minecraft.network.FriendlyByteBuf`,
`net.minecraft.network.VarInt`, `net.minecraft.network.VarLong`,
`net.minecraft.network.Utf8String`, and the `StreamCodec` instances in
`net.minecraft.network.codec.ByteBufCodecs`.

## Numeric primitives

### Boolean
A single byte. `0x00` is `false`, `0x01` is `true`. Decoders should treat any
non-zero value as `true`, but encoders must always emit `0x01` for `true`.

```text
+--------+
| 1 byte |
+--------+
```

### Byte
Signed 8-bit integer. Range: `-128..=127`.

### Unsigned Byte
Unsigned 8-bit integer. Range: `0..=255`. Used by client information packets,
container indices, and similar small enumerations.

### Short
Signed 16-bit big-endian integer. Range: `-32768..=32767`.

### Unsigned Short
Unsigned 16-bit big-endian integer. Range: `0..=65535`. Used for the port
number in [Handshake](./handshake) and a few packet length fields.

### Int
Signed 32-bit big-endian integer. Range: `-2³¹..=2³¹-1`.

### Long
Signed 64-bit big-endian integer. Range: `-2⁶³..=2⁶³-1`.

### Float
IEEE-754 single-precision 32-bit big-endian. NaN is preserved bit-exactly.

### Double
IEEE-754 double-precision 64-bit big-endian. NaN is preserved bit-exactly.

### VarInt
Variable-length encoding of a signed 32-bit integer (1–5 bytes). Each byte uses
the low 7 bits as data and the high bit as a continuation flag. The value is
reconstructed by concatenating the 7-bit groups in little-endian order.

```text
byte_n: c d d d d d d d
        ^ \-----v-----/
        |       7 data bits
        continuation bit (1 = another byte follows)
```

A negative value is sent in the same way as its 32-bit two's complement
unsigned representation, so it always occupies the maximum 5 bytes. The decoder
throws if it has read more than 5 bytes without seeing a cleared continuation
bit.

### VarLong
Variable-length encoding of a signed 64-bit integer (1–10 bytes). Encoded the
same way as a [VarInt](#varint), but with up to ten 7-bit groups.

## String types

### String
Length-prefixed UTF-8 string:

```text
+-----------------+----------------------+
| Length (VarInt) | UTF-8 bytes (Length) |
+-----------------+----------------------+
```

`Length` is the **byte length of the UTF-8 encoding**, not the number of
characters. The default character cap is 32 767; the wire byte cap is therefore
`utf8MaxBytes(32767)` (≈ 98 304 bytes). Specific fields may impose tighter
caps (e.g. player name is 16 chars, server brand 32, signatures 1024).

### Identifier
A namespaced resource location encoded exactly like a [String](#string), of the
form `namespace:path`. Maximum length 32 767 characters. If `namespace:` is
omitted, the client substitutes `minecraft:`.

The `namespace` matches `[a-z0-9_.\-]+` and `path` matches `[a-z0-9_./\-]+`.

### Chat
Refers to a [Text Component](./text-component). On the wire it is the
network NBT encoding of the component value (see [NBT](#nbt)). The plain
form (a String holding a literal) is also accepted.

### JSON Text Component
Legacy form: a String containing a JSON-serialised text component. Used in a
small number of packets retained for compatibility (e.g. the disconnect reason
during the Login state). Maximum length 262 144 characters.

## Identity & position types

### UUID
Two consecutive Longs, most-significant bits first, then least-significant
bits.

```text
+----------+----------+
| msb (8B) | lsb (8B) |
+----------+----------+
```

### Position
Encoded into a single signed 64-bit integer. The current layout packs the
fields as **x (26 bits, signed) || z (26 bits, signed) || y (12 bits,
signed)**:

```text
bit 63                                     bit 0
+--------------------+--------------------+----------+
|     x (26 bits)    |     z (26 bits)    | y (12b)  |
+--------------------+--------------------+----------+
```

Decoded as:

```text
x = val >> 38
y = (val << 52) >> 52       // sign-extend lower 12 bits
z = (val << 26) >> 38       // sign-extend middle 26 bits
```

Encoded as `((x & 0x3FFFFFF) << 38) | ((z & 0x3FFFFFF) << 12) | (y & 0xFFF)`.
Range: `x, z ∈ [-33_554_432, 33_554_431]`, `y ∈ [-2048, 2047]`.

> NOTE: Older protocol versions (pre-1.14) used a different layout
> (`x:26 || y:12 || z:26`). The order shown above (`x || z || y`) is the
> current one and is what 26.1.2 transmits.

### Angle
Single unsigned byte representing an angle in 1/256 of a full turn. To convert
to degrees: `deg = byte * 360 / 256`. Wraps modulo 256.

### LP Vec3
Low-precision 3D vector used since 1.21.7 for entity movement/velocity fields
(`net.minecraft.network.LpVec3`, exposed as `Vec3.LP_STREAM_CODEC`). Variable
length:

- If `|x|, |y|, |z|` are all below `3.051944088384301e-5` (`ABS_MIN`), the
  vector is encoded as a single `0x00` byte.
- Otherwise the encoder computes `scale = ceil(max(|x|,|y|,|z|))`, packs each
  axis into 15 bits as `round((axis/scale * 0.5 + 0.5) * 32766)`, and emits a
  64-bit buffer laid out as:

  ```text
  bits 0..1   : low 2 bits of scale
  bit  2      : continuation flag (set when scale needs > 2 bits)
  bits 3..17  : packed X (15 bits)
  bits 18..32 : packed Y (15 bits)
  bits 33..47 : packed Z (15 bits)
  ```

  Wire layout: 1 byte (bits 0..7), 1 byte (bits 8..15), 4 bytes big-endian
  (bits 16..47) — 6 bytes total. When the continuation flag is set, a VarInt
  carrying the upper bits of the scale (`scale >> 2`) follows.

- Range per axis: `±1.7179869183e10`. `NaN` is sanitised to `0.0`.

This format **replaces** the pre-1.21.7 `Velocity X / Y / Z` triple of signed
shorts. Encoding velocity as three shorts produces the symptom *"packet larger
than I expected, found 5 bytes extra"* on the vanilla client.

## Collections

### Byte Array
Length-prefixed raw bytes:

```text
+-----------------+--------------------+
| Length (VarInt) | bytes (Length)     |
+-----------------+--------------------+
```

The decoder rejects lengths greater than the receiver-imposed cap (often the
remaining readable bytes of the frame).

### Prefixed Array
A length-prefixed array of any element type:

```text
+-----------------+----------------------+
| Length (VarInt) | element[0..Length-1] |
+-----------------+----------------------+
```

Each element is encoded with its own type's codec. Most array fields impose a
documented per-packet maximum size; the decoder throws if exceeded.

### Prefixed Optional
A boolean discriminator followed by the value when present:

```text
+----------------+--------------------------+
| Present (Bool) | Value (only if Present)  |
+----------------+--------------------------+
```

### BitSet
A length-prefixed array of 64-bit longs holding the BitSet's contents in Java's
`BitSet.toLongArray()` little-endian-bit ordering (bit `n` of the set lives in
bit `n & 63` of long `n >> 6`).

```text
+-----------------+-----------------------+
| Length (VarInt) | data: long[Length]    |
+-----------------+-----------------------+
```

### Fixed BitSet
A BitSet of statically known size `N` (agreed out-of-band). No length prefix.
Encoded as `ceil(N / 8)` raw bytes, again following Java's `BitSet.toByteArray()`
ordering. The encoder throws if any bit at index `≥ N` is set.

```text
+--------------------+
| ceil(N/8) bytes    |
+--------------------+
```

## NBT

NBT (*Named Binary Tag*) is Mojang's tagged binary serialisation format. It is
used both on disk (region files, level data) and on the wire, in a slightly
different framing. This section describes the **network NBT** form used by
protocol 26.1.2.

### NBT
The "network NBT" framing used in modern protocol packets:

```text
+-----------+--------------------+
| Type (1B) | Payload (Type-spec)|
+-----------+--------------------+
```

- **Type** is the standard NBT tag id (table below).
- **Payload** is the type-specific bytes (table below).
- **No root name is written.** The legacy empty UTF (`0x00 0x00`) that
  preceded the payload prior to 1.20.2 is omitted on the wire.
- A standalone `TAG_End` (`0x00`) byte represents an absent / null tag and is
  the only legal "empty" value when a packet field is typed as NBT.

The default decoder enforces an `NbtAccounter` quota (≈ 2 MiB and depth 512).
"Trusted" variants used by some server-built payloads lift the limit.

#### Tag types

| ID  | Name | Payload |
|----:|------|---------|
| `0x00` | `TAG_End` | None. Marks the end of a [Compound](#compound) and is the absent-tag sentinel at the root. |
| `0x01` | `TAG_Byte` | One signed [Byte](#byte). |
| `0x02` | `TAG_Short` | One big-endian signed [Short](#short). |
| `0x03` | `TAG_Int` | One big-endian signed [Int](#int). |
| `0x04` | `TAG_Long` | One big-endian signed [Long](#long). |
| `0x05` | `TAG_Float` | One big-endian IEEE-754 [Float](#float). |
| `0x06` | `TAG_Double` | One big-endian IEEE-754 [Double](#double). |
| `0x07` | `TAG_Byte_Array` | `Int` length `N` then `N` raw bytes. |
| `0x08` | `TAG_String` | NBT-string: unsigned 16-bit length `N` (big-endian), then `N` bytes of [modified UTF-8](#modified-utf-8). |
| `0x09` | `TAG_List` | One element-type byte (`TAG_*`), then `Int` length `N` (big-endian, ≥ 0), then `N` payloads of the declared element type concatenated **without** their type bytes or names. An empty list (`N == 0`) is conventionally encoded with element-type `TAG_End`. |
| `0x0A` | `TAG_Compound` | Sequence of *named entries*: `Type byte` + NBT-string `Name` + `Payload`, repeated until a terminating `TAG_End` (`0x00`) byte. See [Compound](#compound). |
| `0x0B` | `TAG_Int_Array` | `Int` length `N` then `N` big-endian signed `Int`s. |
| `0x0C` | `TAG_Long_Array` | `Int` length `N` then `N` big-endian signed `Long`s. |

All NBT integers and floats are big-endian regardless of platform. Lengths
inside `TAG_*_Array` and `TAG_List` are 32-bit signed `Int`s and **must not be
negative**; decoders reject negative lengths.

#### Named entry (inside Compound)

```text
+-----------+--------------------+--------------------+
| Type (1B) | NBT-string Name    | Payload (Type-spec)|
+-----------+--------------------+--------------------+
```

A `Type` of `0x00` (`TAG_End`) terminates the compound and carries no name or
payload. Every other entry inside a compound is *named*; this is the only
context (along with the legacy disk root) in which NBT names appear.

#### Modified UTF-8

NBT strings (`TAG_String` and the `Name` of named entries) use Java's
*modified UTF-8* encoding, **not** the protocol's standard [String](#string)
encoding. The differences are:

- The NUL byte (`U+0000`) is encoded as the two-byte sequence `0xC0 0x80` so
  that no real character produces a `0x00` byte in the stream.
- Supplementary code points (`U+10000`–`U+10FFFF`) are emitted as **two**
  three-byte sequences encoding the UTF-16 surrogate pair, never as a single
  four-byte sequence.

Length is a 16-bit *unsigned* short giving the encoded byte length (max
`65 535`). Implementations decoding NBT strings as standard UTF-8 will
mishandle these two cases.

### Compound
A specialisation of [NBT](#nbt) where the top-level tag is required to be a
`TAG_Compound` (`0x0A`). The wire shape is identical to a generic NBT tag —
one type byte (always `0x0A` when present), then the named-entry list, then
a `TAG_End` (`0x00`) terminator. Used wherever a packet field is declared as
a structured object, e.g. registry entries, recipe payloads, item data
components, chat-message session metadata.

A field typed `Compound` may also be encoded as a single `TAG_End` (`0x00`)
to signal an absent value when the field is documented as optional.

## Vector & geometric helpers

### Vector3f
Three consecutive [Float](#float)s: `x`, `y`, `z`.

### Quaternionf
Four consecutive [Float](#float)s: `x`, `y`, `z`, `w` — in that order.

### ChunkPos
Two `int`s packed into a single Long: high 32 bits = `x`, low 32 bits = `z`
(both signed).

### GlobalPos
A dimension [Identifier](#identifier) followed by a [Position](#position).

### BlockHitResult
[Position](#position), then a `Direction` enum encoded as a VarInt
(0 = down, 1 = up, 2 = north, 3 = south, 4 = west, 5 = east), then three
floats `(cx, cy, cz)` representing the click point relative to the block's
minimum corner, then two booleans `(inside, worldBorder)`.

## Registry-typed values

A registry value (block, item, entity type, …) is encoded as a single
[VarInt](#varint) holding its id within the receiver's view of that registry.
The actual id table is established during the Configuration state via
`ClientboundRegistryDataPacket` — see [Registries](./registries).

A `Holder<T>` is encoded with a 1-based id: id `0` means "direct value"
(followed by the value's payload), and id `n ≥ 1` means "reference to entry
`n - 1` of the registry."

A `HolderSet<T>` is encoded as a VarInt `n`. If `n == 0`, an Identifier
follows naming a tag. Otherwise the set contains `n - 1` Holders.
