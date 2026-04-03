# Networking & Protocol

## Protocol States

Void implements the Minecraft protocol state machine with six states:

```rust
pub enum State {
    Handshake     = 0x0,
    Status        = 0x1,
    Login         = 0x2,
    Transfer      = 0x3,
    Configuration = 0x4,
    Play          = 0x5,
}
```

The active state determines which packets are valid. Each client entity tracks its state via the `ConnectionState` component.

## Connection Lifecycle

### Full Join Flow

```
Client                              Server
  |                                    |
  |--- TCP Connect ------------------->|  Server::accept()
  |                                    |  Spawn Client task
  |                                    |
  |--- Handshake (next_state=2) ------>|  Store ProtocolVersion
  |                                    |  Set state -> Login
  |                                    |
  |--- LoginStart (name, uuid) ------->|  Store PlayerName, PlayerUuid
  |<-- LoginSuccess -------------------|
  |                                    |
  |--- LoginAcknowledged ------------->|  Set state -> Configuration
  |<-- KnownPacks ---------------------|
  |                                    |
  |--- ClientInformation ------------->|  Store ClientSettings
  |--- PluginMessage (brand) --------->|  (logged)
  |--- KnownPacks -------------------->|  Send all RegistryData
  |<-- RegistryData (xN) -------------|
  |<-- FinishConfiguration ------------|
  |                                    |
  |--- FinishConfigurationAck -------->|  Set state -> Play
  |                                    |  Allocate MinecraftEntityId
  |                                    |  Insert all player components
  |<-- Play Login ---------------------|  (game settings, dimensions)
  |<-- Commands -----------------------|  (command tree for tab-completion)
  |<-- GameEvent(StartWaiting) --------|
  |<-- SetCenterChunk -----------------|
  |<-- ChunkDataAndLight (xN) --------|  (initial_chunk_radius)
  |<-- SynchronizePlayerPosition ------|  (teleport to spawn)
  |                                    |  Trigger PlayerJoinEvent
  |                                    |
  |--- ConfirmTeleportation ---------->|  Clear TeleportState.pending_id
  |--- PlayerLoaded ------------------>|  Insert PlayerReady
  |                                    |  Trigger PlayerReadyEvent
  |                                    |  Broadcast spawn to other players
  |                                    |
  |<== Keep-alive (periodic) =========|
  |=== Keep-alive response ==========>|
  |                                    |
  |<== Chunk streaming ===============|  (as player moves)
  |<== Position broadcasts ===========|  (from other players)
```

### Server List Ping

When a client connects with `next_state = 1` (Status):

```
Client                              Server
  |--- Handshake (next_state=1) ------>|
  |--- StatusRequest ----------------->|
  |<-- StatusResponse -----------------|  (MOTD, version, player count)
  |--- PingRequest (timestamp) ------->|
  |<-- PingResponse (timestamp) ------|
```

## Packet Handling Pipeline

On every tick during `PreUpdate`:

1. **Drain** all packets from the incoming channel
2. For each packet:
   - Look up or create the client entity in `ClientToEntityMap`
   - Read the entity's `ConnectionState`
   - Decode the raw packet bytes using the protocol crate
   - Call the appropriate `handle_{state}_packet()` function
3. **Drain** the disconnect channel and despawn disconnected entities

```rust
fn dispatch_packet(world, client_id, entity, packet) {
    match state {
        Handshake     => handlers::handshake::handle_handshake_packet(..),
        Status        => handlers::status::handle_status_packet(..),
        Login         => handlers::login::handle_login_packet(..),
        Configuration => handlers::configuration::handle_configuration_packet(..),
        Play          => handlers::play::handle_play_packet(..),
    }
}
```

## Keep-Alive System

The server sends periodic keep-alive packets to detect dead connections:

- **Interval**: 200 ticks (10 seconds at 20 TPS)
- **Mechanism**: Each tick, `KeepAliveTicker` increments. When the interval is reached, a `KeepAlive` packet with a timestamp ID is sent to all ready players.
- **Tracking**: `KeepAliveState` on each player tracks the last sent ID and whether a response is pending.
- **Timeout**: If a player hasn't responded when the next keep-alive is due, a warning is logged and the keep-alive is skipped for that player.

## Status Response

When a client pings the server list, the response includes:

| Field | Source |
|---|---|
| Version name | `"Void Server"` |
| Protocol version | Client's declared version (echoed back) |
| Max players | `ServerConfig::max_players` |
| Online players | `0` (hardcoded) |
| Description (MOTD) | `ServerConfig::motd` |

## Current Limitations

- **No encryption**: The server does not implement Mojang authentication or encrypted connections. All traffic is plaintext.
- **No compression**: Packet compression is not implemented. All packets are sent at full size.
- **No Transfer state**: The Transfer protocol state is defined but not handled.
