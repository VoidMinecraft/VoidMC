# Connection Lifecycle

A Minecraft connection moves through a fixed set of *protocol states*. Each
state defines its own clientbound and serverbound packet tables; packet ids
are local to a `(state, direction)` pair.

The states are enumerated in `net.minecraft.network.ConnectionProtocol`:

- `HANDSHAKING` (`handshake`)
- `STATUS` (`status`)
- `LOGIN` (`login`)
- `CONFIGURATION` (`configuration`)
- `PLAY` (`play`)

The direction is `net.minecraft.network.protocol.PacketFlow`:

- `SERVERBOUND` — client → server.
- `CLIENTBOUND` — server → client.

The packet tables live in
`HandshakeProtocols`, `StatusProtocols`, `LoginProtocols`,
`ConfigurationProtocols`, and `GameProtocols`.

## State machine

```text
            +-------------+
            | HANDSHAKING |
            +------+------+
                   |
       intent = 1  |  intent = 2 or 3
                   |
       +-----------+-----------+
       |                       |
       v                       v
   +--------+            +-----------+
   | STATUS |            |   LOGIN   |
   +--------+            +-----+-----+
                               |
                    Login Acknowledged
                               |
                               v
                       +---------------+
                       | CONFIGURATION |  <-----+
                       +-------+-------+        |
                               |                |
                       Finish Configuration     |
                               |                |
                               v                |
                          +--------+            |
                          |  PLAY  |--- Start --+
                          +--------+   Configuration (S→C)
```

Once a transition fires, the previous state's packet table no longer applies.
A client that receives an out-of-state packet treats it as a fatal protocol
error.

## Handshake

A new connection always starts in `HANDSHAKING`. The client sends exactly one
packet:

### Serverbound: Intention (a.k.a. Handshake)

`HandshakePacketTypes.CLIENT_INTENTION`. Fields:

| Field             | Type           | Notes                                       |
|-------------------|----------------|---------------------------------------------|
| Protocol Version  | VarInt         | Server compares against its supported set.  |
| Server Address    | String (≤255)  | Hostname the client believes it is reaching. |
| Server Port       | Unsigned Short | TCP port (informational).                   |
| Next State        | VarInt         | `1` STATUS, `2` LOGIN, `3` TRANSFER.        |

The server immediately switches to the requested state. There is no
clientbound handshake packet.

`TRANSFER` is encoded the same as `LOGIN` and proceeds through the Login
state identically; the server's `ServerLoginPacketListenerImpl` may use the
intent to allow a "transferred" connection (sent via the Play state's
`ClientboundTransferPacket` from another server) to bypass certain checks.

## Status

Used by the server-list ping. Two clientbound packets, two serverbound.

Canonical exchange:

```text
C -> S  Status Request                (empty)
S -> C  Status Response               (JSON document)
C -> S  Ping Request                  (Long payload)
S -> C  Pong Response                 (echoes the Long)
```

The server may close the connection at any point after responding. Either
ping or status may be omitted by the client.

## Login

Used to authenticate the player and to negotiate compression and
custom-payload extensions. Canonical successful flow:

```text
C -> S  Hello                         (username, profile UUID)
S -> C  Hello                         (server id, public key, verify token,
                                       should authenticate)
C -> S  Key                           (encrypted shared secret + verify token)
        --- both sides enable AES/CFB8 encryption from the next byte ---
S -> C  Login Compression  [optional] (threshold)
        --- both sides enable zlib framing from the next packet ---
S -> C  Login Finished                (resolved game profile)
C -> S  Login Acknowledged            (empty; switches state)
        --- both sides switch to CONFIGURATION ---
```

Side packets that may interleave:

- `S -> C  Custom Query` / `C -> S  Custom Query Answer` — channel-tagged
  arbitrary payload.
- `S -> C  Cookie Request` / `C -> S  Cookie Response` — read a
  client-side persistent cookie.
- `S -> C  Login Disconnect` — terminates the connection. The reason is a
  [JSON Text Component](./data-types#json-text-component) (legacy String
  form), not the NBT form used elsewhere.

Encryption is mandatory for online-mode servers and optional for offline
mode. When enabled, **all** subsequent bytes — including the `Login
Compression` packet itself — pass through the AES/CFB8 layer.

When the client receives `Login Finished`, it must respond with `Login
Acknowledged`. The server transitions to `CONFIGURATION` upon reading that
packet; the client transitions immediately upon sending it.

## Configuration

Used to push registries, tags, resource packs, server brand and any other
"static" world data before play begins. Either side may interleave its
packets freely; the canonical order in a vanilla join is:

```text
S -> C  Custom Payload   "minecraft:brand"
S -> C  Update Enabled Features
S -> C  Select Known Packs                       (advertises built-in packs)
C -> S  Select Known Packs                       (acknowledges which it has)
S -> C  Registry Data                            (one packet per registry,
                                                  excluding entries the client
                                                  already knows from packs)
S -> C  Update Tags
S -> C  Finish Configuration
C -> S  Acknowledge Finish Configuration         (empty)
        --- both sides switch to PLAY ---
```

Side packets allowed during configuration:

- `Client Information` (C → S) — locale, view distance, chat mode, displayed
  skin layers, main hand, text filtering, allow server listings, particle
  status. Sent at least once before play.
- `Custom Payload` (both directions) — channel + arbitrary bytes.
- `Cookie Request` / `Cookie Response`.
- `Resource Pack Push` / `Resource Pack Pop` / `Resource Pack` (status).
- `Keep Alive` / `Ping` / `Pong`.
- `Disconnect` (S → C; reason is a [Text Component](./text-component)).
- `Store Cookie`, `Transfer`, `Update Tags`, `Custom Report Details`,
  `Server Links`, `Show Dialog`, `Clear Dialog`, `Code Of Conduct`,
  `Accept Code Of Conduct`.

The `Finish Configuration` / `Acknowledge Finish Configuration` round trip
is what actually changes the state.

## Play

Carries gameplay. Canonical post-join exchange (after Configuration):

```text
S -> C  Login (Play)                  (entity id, gamemode, dimension list,
                                       world info, …)
S -> C  Change Difficulty
S -> C  Player Abilities
S -> C  Set Held Slot
S -> C  Update Recipes / Update Tags    [if not sent in configuration]
S -> C  Entity Event(s) for the player
S -> C  Commands                       (the command tree)
S -> C  Player Info Update             (own profile, then others)
S -> C  Initialize World Border
S -> C  Set Default Spawn Position
S -> C  Set Time
S -> C  Set Container Content          (inventory)
S -> C  Set Health, Set Experience
S -> C  Synchronize Player Position    (forces the client to confirm)
C -> S  Confirm Teleportation          (echoes the teleport id)
S -> C  Set Center Chunk
S -> C  Chunk Batch Start
S -> C  Level Chunk With Light  (× N)
S -> C  Chunk Batch Finished           (count of chunks delivered)
C -> S  Chunk Batch Received           (smoothing rate feedback)
```

From this point onward both sides exchange packets freely. `Keep Alive`
must be answered within 30 s or the connection is dropped.

### Re-entering Configuration

A server may move a *playing* connection back into the Configuration state at
any time (for example to push a new resource pack or a different registry
view):

```text
S -> C  Start Configuration            (PLAY -> ?)
        --- server sets its outbound protocol to CONFIGURATION immediately ---
        --- client must finish processing any in-flight PLAY packets and
            respond when ready ---
C -> S  Configuration Acknowledged     (PLAY serverbound)
        --- both sides are now in CONFIGURATION ---
... configuration sequence as above, ending with Finish Configuration /
    Acknowledge Finish Configuration ...
```

The server's outbound switch happens as soon as it sends `Start
Configuration`; the client does **not** ack the state change until its play
queue is drained, and the inbound switch on the server side happens on
receipt of `Configuration Acknowledged`. During the gap the server treats any
PLAY packet from the client normally.

### Transfer

The clientbound `Transfer` packet is available in both Configuration and
Play. It instructs the client to disconnect and reconnect to a different
host/port. The new connection re-runs Handshake → Login with intent =
`TRANSFER` (id 3); a server that understands transfer may then skip
authentication based on a previously stored cookie.
