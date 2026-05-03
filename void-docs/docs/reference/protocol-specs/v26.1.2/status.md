# Status State

The Status state is reached when a client sends a [Handshake](./handshake) with `Next State = 1`. It implements the multiplayer server-list "ping": the client requests a JSON description of the server (MOTD, player counts, version, favicon) and optionally a round-trip latency measurement. After the latency exchange the connection is closed by the client.

See [Data Types](./data-types) for primitive encodings and [Text Component](./text-component) for the structure of the `description` field.

| ID | Name | Direction |
|----|------|-----------|
| `0x00` | [Status Request](#status-request) | Server-bound |
| `0x01` | [Ping Request](#ping-request) | Server-bound |
| `0x00` | [Status Response](#status-response) | Client-bound |
| `0x01` | [Pong Response](#pong-response) | Client-bound |

---

## Server-bound

### Status Request

**Packet ID:** `0x00` · **State:** Status · **Bound To:** Server

This packet has no fields.

**Semantics.** Sent first by the client after entering the Status state. The server replies with exactly one [Status Response](#status-response). Sending more than one Status Request on the same connection is a protocol error.

### Ping Request

**Packet ID:** `0x01` · **State:** Status · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Payload | [Long](./data-types#long) | An opaque value chosen by the client (typically the current Unix time in milliseconds). The server must echo it back unmodified. |

**Semantics.** Sent by the client after it has received the Status Response. The server immediately replies with [Pong Response](#pong-response) carrying the same payload, which lets the client compute round-trip latency. The connection is closed by the client after the pong arrives.

---

## Client-bound

### Status Response

**Packet ID:** `0x00` · **State:** Status · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| JSON Response | [String](./data-types#string) (32767) | A UTF-8 JSON document describing the server. See the schema below. |

**JSON schema.** The string is a JSON object with the following optional fields:

| Field | Type | Notes |
|-------|------|-------|
| `description` | [Text Component](./text-component) (JSON form) | The MOTD shown in the server list. Defaults to an empty component if omitted. |
| `players` | Object | `{ "max": int, "online": int, "sample": [{ "name": String, "id": UUID-String }] }`. The optional `sample` populates the hover tooltip. |
| `version` | Object | `{ "name": String, "protocol": int }`. `protocol` is the server's wire version (`775` for 26.1.2); the client compares it against its own to decide whether to render an "Outdated" / "Incompatible" badge. |
| `favicon` | [String](./data-types#string) | A 64×64 PNG icon encoded as `data:image/png;base64,…`. |
| `enforcesSecureChat` | [Boolean](./data-types#boolean) | When `true`, the server requires Mojang-signed chat messages. Vanilla 1.19+ clients display a warning if the user joins an `enforcesSecureChat=false` server while their profile is signed-in. Defaults to `false`. |

**Semantics.** Sent in reply to [Status Request](#status-request). Only one is sent per connection.

### Pong Response

**Packet ID:** `0x01` · **State:** Status · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Payload | [Long](./data-types#long) | The exact value received in [Ping Request](#ping-request). |

**Semantics.** Sent in reply to a Ping Request. Implementations must not modify the payload — the client uses it both to compute latency and to correlate the response.
