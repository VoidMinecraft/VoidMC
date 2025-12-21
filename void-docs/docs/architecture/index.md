# Architecture Overview

Void is built with a modular, layered architecture that emphasizes separation of concerns and type safety.

## System Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                   Minecraft Client                          │
└─────────────────────────────────────────────────────────────┘
                            ↓ TCP
┌─────────────────────────────────────────────────────────────┐
│         void-net: Async TCP Socket Wrapper                  │
│              (Tokio-based networking)                       │
└─────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────┐
│   void-codec: Binary Serialization/Deserialization         │
│              (Encode/Decode Traits)                         │
└─────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────┐
│  void-protocol: Minecraft Protocol Definitions              │
│   (Packets organized by state and direction)                │
└─────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────┐
│          void: Core Server Application                       │
│  (Client handling, game logic, state management)            │
└─────────────────────────────────────────────────────────────┘
```

## Crate Structure

### 1. **void** - Main Server Application

The core server that orchestrates all other components.

**Key modules:**

- `server.rs`: Server lifecycle and TCP listener
- `client.rs`: Per-client connection handler
- `game.rs`: Shared game state and logic

**Responsibilities:**

- Accept incoming client connections
- Manage protocol state transitions
- Dispatch packets to appropriate handlers
- Maintain shared game state

### 2. **void-protocol** - Protocol Definitions

Defines all Minecraft protocol packets organized by connection state.

**Organization:**

```
void-protocol/src/
├── lib.rs              # State enum, exports
├── clientbound.rs      # Server → Client packets
├── clientbound/
│   ├── status/         # Status state packets
│   ├── login/          # Login state packets
│   ├── configuration/  # Configuration state packets
│   └── play/           # Play state packets
└── serverbound/        # Client → Server packets
```

**Key concepts:**

- Tagged enums with packet IDs using `#[codec(packet_id = 0xXX)]`
- State-specific packet types ensure type safety
- Direction routing (clientbound vs serverbound)

### 3. **void-codec** - Binary Serialization

Low-level binary encoding and decoding primitives.

**Key traits:**

```rust
pub trait Encode {
    fn encode(&self, buf: &mut Vec<u8>);
}

pub trait Decode: Sized {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError>;
}
```

**Supported types:**

- Primitive types: `bool`, `u8`, `u16`, `u32`, `u64`, etc.
- Variable-length integers: `VarI32`, `VarI64`
- Complex types: `UUID`, Strings, Lists
- Custom types via derive macros

### 4. **void-codec-macros** - Procedural Macros

Compile-time code generation for encoding/decoding.

**Macros:**

- `#[derive(Encode, Decode)]`: Auto-implements traits
- `#[codec(varint32)]`: Apply variable-length encoding
- `#[codec(json)]`: Serde JSON serialization
- `#[codec(tagged)]`: Tagged enum dispatch

### 5. **void-net** - Networking Layer

Async TCP socket abstraction on top of Tokio.

**Responsibilities:**

- Non-blocking socket operations
- Connection lifecycle management
- Raw read/write interfaces for protocol layer

## Connection State Machine

Minecraft connections flow through predefined states:

```
┌──────────┐
│ Handshake │  Client announces intent (status, login, or ping)
└────┬─────┘
     │
     ↓
┌──────────┐
│  Status  │  Server responds with MOTD and player count
└────┬─────┘
     │
     ↓
┌──────────┐
│  Login   │  Authentication and encryption setup
└────┬─────┘
     │
     ↓
┌──────────┐
│   Config │  Server sends world data and settings
└────┬─────┘
     │
     ↓
┌──────┐
│ Play │  Full game synchronization and interaction
└──────┘
```

**State transitions in code:**

```rust
pub enum State {
    Handshake = 0x0,
    Status = 0x1,
    Login = 0x2,
    Configuration = 0x4,
    Play = 0x5,
}
```

## Packet Flow Example

### Connecting a Client

1. **Client connects** → TCP handshake with void-net
2. **Handshake packet received** → Decoded by void-codec
3. **Route to state machine** → void-protocol validates state
4. **Handle in client.rs** → Update state to `Status` or `Login`
5. **Send response** → Encode response packet, send via void-net

### Processing a Play Packet

```
Raw bytes from client
    ↓
[void-codec] Decode VarInt packet ID
    ↓
[void-protocol] Match ID to PlayPacket enum
    ↓
[void-protocol] Deserialize packet data
    ↓
[void:client.rs] Handle specific packet type
    ↓
[void:game.rs] Update game state
    ↓
[void-codec] Encode response packet
    ↓
Send bytes to client
```

## Type Safety & Validation

### Compile-time Guarantees

- **Packet IDs**: Define once, checked at compilation
- **Field ordering**: Maintained through struct definition order
- **Encoding strategy**: Specified via attributes, verified by macros
- **State transitions**: Type system enforces valid state changes

### Runtime Validation

- **DecodeError**: Explicit error types for invalid data
- **Packet filtering**: Only packets valid for current state are processed
- **Boundary checks**: Buffer overruns prevented with size checks

## Performance Considerations

### Async Architecture

- **Tokio-based**: Non-blocking I/O for thousands of concurrent clients
- **Spawn-per-client**: Each client gets its own async task
- **Shared state**: `Arc<Mutex<Game>>` for safe concurrent access

### Memory Efficiency

- **VarInt encoding**: Saves bytes for small integers (common in Minecraft)
- **String pooling**: Potential for future optimization
- **Zero-copy reading**: Protocol decoder works with borrowed buffers

### Optimizations

- **Packet batching**: Future: combine multiple packets per frame
- **Spatial hashing**: Future: efficient entity location queries
- **View culling**: Future: only send packets for visible entities

## Configuration & Customization

### Server Configuration

Located in [void/src/game.rs](../../void/src/game.rs):

- MOTD (Message of the Day)
- Favicon
- Player limits
- Difficulty, mode, etc.

### Protocol Versioning

Currently targets **Minecraft 1.21.4** protocol. Update the version constant when:

- New packet types are added
- Packet IDs change
- Protocol structure changes

## Future Scalability

The modular design supports:

- **Plugin system**: Protocol could route to custom handlers
- **World persistence**: Game state can integrate with databases
- **Clustering**: Multiple server instances with shared state
- **Performance optimization**: Each layer can be independently optimized
