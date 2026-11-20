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
  |                                    |  (or, server full: LoginDisconnect, close)
  |<-- SetCompression (threshold) -----|  (network thread, if enabled)
  |<-- LoginSuccess -------------------|  (compressed framing from here on)
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
  |<-- EntityEvent --------------------|  (permission level: 28 operator, 24 not)
  |<-- Commands -----------------------|  (the commands this player may run)
  |<-- GameEvent(StartWaiting) --------|
  |<-- SetCenterChunk -----------------|
  |<-- ChunkBatchStart ----------------|
  |<-- ChunkDataAndLight (xN) --------|  (initial_chunk_radius)
  |<-- SynchronizePlayerPosition ------|  (teleport to spawn, after the 3x3 centre)
  |<-- ChunkBatchFinished(N) ----------|
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
  |--- ChunkBatchReceived ------------>|  (once per batch; rate ignored)
  |<== Chunk streaming ===============|  (as player moves, one batch per tick)
  |<== Position broadcasts ===========|  (from other players)
```

Until the client reaches the play state, the [pre-play timeout](#pre-play-timeout)
applies; from then on, [keep-alives](#keep-alive-system) do, whether or not the
client has sent `PlayerLoaded`.

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

## Compression

When `ServerConfig::compression_threshold` is set (default `Some(256)`), the
connection's writer sends `SetCompression` right before the first
`LoginSuccess` and switches both halves of the socket to compressed framing:

```
VarInt packet_length | VarInt data_length | payload
```

`data_length = 0` means the payload is uncompressed (packet below the
threshold); otherwise the payload is zlib data inflating to exactly
`data_length` bytes. The writer flips a flag shared with the reader right after
`SetCompression` is buffered, before it is flushed, so the client cannot have
received it yet; the reader samples the flag after each frame's length prefix,
so the client's first compressed packet (`LoginAcknowledged`) is always parsed
as compressed. Game code never sees compression: it keeps sending
`LoginSuccess` through `Players`.

## Packet Handling Pipeline

On every tick during `PreUpdate`:

1. **Drain** all packets from the incoming channel
2. For each packet:
   - Look up or create the client entity in `ClientToEntityMap` (a new entity takes over the client's outbound sender in `ClientSenders`)
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

### Undecodable Packets

A packet that fails to decode is handled in one place, `ingest_network_packets`,
the same way in every state; which case applies is decided from its id, through
the `Tagged` trait of the state's packet enum (an invalid enum value inside a
known packet fails with the same error as an unknown id, so the error alone
cannot tell them apart):

| Packet | What happens | Counted in |
|---|---|---|
| An id VoidMC has no decoder for in the connection's state | Dropped with a WARN (`Unrecognized packet from client …`); the client stays connected. VoidMC does not decode every packet a vanilla client sends, so these are not the client's fault. | `voidmc_packets_unrecognized_total` |
| A known id whose body does not decode: an invalid enum value, a truncated body, trailing bytes, a string or collection over its decode limit, … | The client is disconnected, like vanilla, with the reason `Internal Exception: Failed to decode packet '<name>' (0x<id>)`, sent in its state's disconnect packet; the connection is closed once it is written. Logged at INFO, since it is the client's fault, not the server's. | `voidmc_packet_errors_total` and `voidmc_clients_kicked_total` |

The disconnect packet is the play `Disconnect`, the configuration `Disconnect` or
the `LoginDisconnect`; a malformed handshake has none, so its connection is just
closed. Status requests are answered on the network thread, which closes a
connection whose status packet does not decode (`voidmc_connection_errors_total{kind="decode"}`).

## Keep-Alive System

The server sends periodic keep-alive packets to detect dead connections:

- **Interval**: 200 ticks (10 seconds at 20 TPS)
- **Mechanism**: Each tick, `KeepAliveTicker` increments. When the interval is reached, a `KeepAlive` packet with a timestamp ID is sent to every client in the play state, ready or not: a client that never sends `PlayerLoaded` is kept alive and timed out like any other.
- **Tracking**: `KeepAliveState`, inserted when the client enters the play state, tracks the last sent ID and whether a response is pending.
- **Timeout**: If a player hasn't responded when the next keep-alive is due, a warning is logged and the keep-alive is skipped for that player. Once a keep-alive has gone unanswered for 30 seconds (`KEEP_ALIVE_TIMEOUT_MS`), the player receives a `Disconnect` ("Timed out") and the network thread closes the connection, which despawns the player like any other disconnect.

## Pre-Play Timeout

The configuration state has no keep-alive exchange. Instead, a connection must
reach the play state within 30 seconds (wall clock) of being accepted, through
handshake, status, login and configuration together, like vanilla's login
time limit. Past it, the server closes the connection:

| State when the time runs out | What the client is told |
|---|---|
| Login | `LoginDisconnect`, vanilla's `multiplayer.disconnect.slow_login` ("Took too long to log in") |
| Configuration | The configuration `Disconnect`, same reason |
| Handshake (or nothing sent at all), status | Nothing: these states have no disconnect packet, the connection is just closed |

A client that passed login holds a `max_players` slot, so a stalled one frees it
when it is closed. The limit is the public `PrePlayTimeout` resource
(`voidmc::network::PrePlayTimeout`, default `PrePlayTimeout(Duration::from_secs(30))`);
change it like `KeepAliveTicker`, for example in tests:

```rust
world.resource_mut::<PrePlayTimeout>().0 = Duration::from_secs(5);
```

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
- **No Transfer state**: The Transfer protocol state is defined but not handled.
