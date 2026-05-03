# Handshake State

The Handshake state is the initial state of every Minecraft Java connection. It exists solely to let the client tell the server which protocol version it speaks and which subsequent state ([Status](./status), [Login](./login), or transfer-resumed Login) it wishes to enter. The Handshake state is *unidirectional* — the server never sends a packet back in this state. Once the server has processed the single serverbound packet it switches its connection state and waits for the next packet in the requested state.

See [Connection Lifecycle](./connection-lifecycle) for the full state diagram and [Data Types](./data-types) for primitive encodings used below.

| ID | Name | Direction |
|----|------|-----------|
| `0x00` | [Intention (Handshake)](#intention-handshake) | Server-bound |

---

### Intention (Handshake)

**Packet ID:** `0x00` · **State:** Handshake · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Protocol Version | [VarInt](./data-types#varint) | The protocol version the client speaks. For Minecraft 1.21.10 (release `26.1.2`) this is `775`. Servers that disagree should respond with a [Login Disconnect](./login#disconnect-login) explaining the mismatch (or, in Status, simply advertise their own version in the response). |
| Server Address | [String](./data-types#string) (255) | The hostname or IP address the client used in its connection string, as a UTF-8 string up to 255 bytes long. Vanilla servers ignore this field; proxies (BungeeCord, Velocity, etc.) often inspect it for forwarding metadata. |
| Server Port | [Unsigned Short](./data-types#unsigned-short) | The TCP port the client connected to. Like Server Address, this is informational; the actual TCP socket is already established. |
| Next State | [VarInt](./data-types#varint) Enum | The state the server should switch to once this packet is processed: `1` = Status, `2` = Login, `3` = Transfer. `Transfer` is identical to `Login` on the wire but signals that this connection was initiated by a previous server's [Transfer](./configuration#transfer) packet, allowing cookies set with [Store Cookie](./configuration#store-cookie) to be retrieved by the new server. |

**Semantics.** Sent by the client immediately after the TCP connection is established and before any other packet. It is a *terminal* packet for the Handshake state: after sending it the client transitions to the requested state without waiting for any acknowledgement, and the server does the same upon decoding. Implementations must never send any other packet in the Handshake state — the protocol contains no other handshake-state packets in either direction.
