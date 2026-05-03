# Binary Codec

The `void-codec` crate provides binary serialization for the Minecraft protocol. It includes `Encode`/`Decode` traits, derive macros, and implementations for all primitive types used by the protocol.

## Encode & Decode Traits

```rust
pub trait Encode {
    fn encode(&self, buf: &mut Vec<u8>);
}

pub trait Decode: Sized {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError>;
}
```

- `Encode::encode` appends bytes to the buffer.
- `Decode::decode` reads bytes from the front of the slice, advancing the slice reference.

## Derive Macros

The `void-codec-macros` crate provides `#[derive(Encode, Decode)]` for automatic implementation:

```rust
use voidmc_codec::{Encode, Decode};

#[derive(Encode, Decode)]
pub struct LoginStart {
    pub name: String,
    pub uuid: uuid::Uuid,
}
```

Fields are encoded/decoded in declaration order using their type's `Encode`/`Decode` implementation.

## Field Attributes

### `#[codec(varint32)]`

Encode an `i32` field as a variable-length integer (1-5 bytes) instead of fixed 4 bytes:

```rust
#[derive(Encode, Decode)]
pub struct MyPacket {
    #[codec(varint32)]
    pub length: i32,        // Variable-length encoding
    pub data: u8,           // Fixed 1-byte encoding
}
```

### `#[codec(json)]`

Encode/decode a field as a JSON string (serialize with serde, then encode as a Minecraft string).

### `#[codec(fixed_length)]`

Encode a `Vec<T>` without a length prefix. The length is determined by a literal value, another field, or an arithmetic expression:

```rust
#[derive(Encode, Decode)]
pub struct ChunkSection {
    pub count: u32,
    #[codec(fixed_length = count)]          // Length from another field
    pub blocks: Vec<u8>,
}

#[derive(Encode, Decode)]
pub struct FixedBuffer {
    #[codec(fixed_length = 16)]             // Literal length
    pub data: Vec<u8>,
}

#[derive(Encode, Decode)]
pub struct Computed {
    pub width: u32,
    pub height: u32,
    #[codec(fixed_length = width * height)] // Arithmetic expression
    pub pixels: Vec<u8>,
}
```

Without this attribute, `Vec<T>` is encoded with a VarI32 length prefix followed by the elements.

With `fixed_length`, `Vec<u8>` uses an optimized bulk copy path (not element-by-element decode).

### `#[codec(remaining)]`

Consume all remaining bytes in the buffer as a `Vec<u8>`. No length prefix is encoded or expected:

```rust
#[derive(Encode, Decode)]
pub struct PluginMessage {
    pub channel: String,
    #[codec(remaining)]
    pub data: Vec<u8>,      // Everything after channel
}
```

Must be on the last field. During decode, all remaining bytes are consumed.

## Enum Attributes

### Tagged Enums (`#[codec(tagged)]`)

For enums where each variant wraps a different packet type. The tag is a VarI32 prefix:

```rust
#[derive(Encode, Decode)]
#[codec(tagged)]
pub enum PlayPacket {
    #[codec(packet_id = 0x00)]
    ConfirmTeleportation(ConfirmTeleportation),
    #[codec(packet_id = 0x1C)]
    SetPlayerPos(SetPlayerPos),
    // ...
}
```

Encoding writes the `packet_id` as a VarI32, then the variant's payload. Decoding reads the VarI32 tag and dispatches to the matching variant.

### Repr Enums

For simple enums with integer discriminants. The enum is encoded as its underlying integer type:

```rust
#[derive(Encode, Decode)]
#[repr(u8)]
pub enum GameMode {
    Survival = 0,
    Creative = 1,
    Adventure = 2,
    Spectator = 3,
}
```

Supports `#[repr(u8)]`, `#[repr(i32)]`, and other integer repr types.

Add `#[codec(varint32)]` to encode a `#[repr(i32)]` enum as a variable-length integer:

```rust
#[derive(Encode, Decode)]
#[codec(varint32)]
#[repr(i32)]
pub enum CompressedEnum {
    Small = 0,      // 1 byte
    Large = 268435455,  // 4 bytes
}
```

## Primitive Types

Built-in `Encode`/`Decode` implementations:

| Type | Encoding |
|---|---|
| `u8`, `i8` | 1 byte |
| `u16`, `i16` | 2 bytes, big-endian |
| `u32`, `i32` | 4 bytes, big-endian |
| `u64`, `i64` | 8 bytes, big-endian |
| `u128`, `i128` | 16 bytes, big-endian |
| `f32` | 4 bytes, IEEE 754, big-endian |
| `f64` | 8 bytes, IEEE 754, big-endian |
| `bool` | 1 byte (`0x00` or `0x01`) |
| `String` | VarI32 length prefix + UTF-8 bytes |
| `VarI32` | 1-5 bytes, variable-length encoding |
| `VarI64` | 1-10 bytes, variable-length encoding |
| `Uuid` | 16 bytes (128-bit, big-endian) |
| `Nbt` (ussr_nbt) | NBT binary format |
| `Vec<T>` | VarI32 length prefix + elements |
| `Option<T>` | 1-byte bool prefix + value if present |

## DecodeError

```rust
pub enum DecodeError {
    UnexpectedEof,              // Not enough bytes to read
    InvalidVarintLength,        // VarInt exceeded maximum length
    InvalidPacketId(Option<u8>),// Unknown tag/discriminant
    InvalidLength,              // Length value is invalid
}
```

## Creating Custom Protocol Packets

Define a new packet struct and derive the codec:

```rust
use voidmc_codec::{Encode, Decode};

#[derive(Encode, Decode)]
pub struct MyCustomPacket {
    #[codec(varint32)]
    pub entity_id: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub message: String,
    #[codec(remaining)]
    pub extra_data: Vec<u8>,
}
```

The derive macros generate the serialization code at compile time, ensuring zero-cost abstraction with no runtime reflection.
