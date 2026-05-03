# Login State

The Login state is reached when a client sends a [Handshake](./handshake) with `Next State = 2` (or `3` for transfer). During Login the server authenticates the player against the Mojang session servers (in online mode), optionally enables packet compression, optionally exchanges plugin-defined query payloads, and finally hands the client a `GameProfile`. The state ends when the client sends [Login Acknowledged](#0x03---login-acknowledged), at which point both sides switch to [Configuration](./configuration).

See [Data Types](./data-types) for primitive encodings, [Connection Lifecycle](./connection-lifecycle) for the surrounding state diagram, and [Text Component](./text-component) for the `reason` field of Disconnect.

| ID | Name | Direction |
|----|------|-----------|
| `0x00` | [Disconnect (Login)](#0x00---disconnect-login) | Client-bound |
| `0x01` | [Hello (Encryption Request)](#0x01---hello-encryption-request) | Client-bound |
| `0x02` | [Game Profile (Login Success)](#0x02---game-profile-login-success) | Client-bound |
| `0x03` | [Login Compression (Set Compression)](#0x03---login-compression-set-compression) | Client-bound |
| `0x04` | [Custom Query (Login Plugin Request)](#0x04---custom-query-login-plugin-request) | Client-bound |
| `0x05` | [Cookie Request](#0x05---cookie-request) | Client-bound |
| `0x00` | [Hello (Login Start)](#0x00---hello-login-start) | Server-bound |
| `0x01` | [Key (Encryption Response)](#0x01---key-encryption-response) | Server-bound |
| `0x02` | [Custom Query Answer (Login Plugin Response)](#0x02---custom-query-answer-login-plugin-response) | Server-bound |
| `0x03` | [Login Acknowledged](#0x03---login-acknowledged) | Server-bound |
| `0x04` | [Cookie Response](#0x04---cookie-response) | Server-bound |

---

## Authentication & Encryption Flow

The Login state implements an asymmetric-key key-exchange that yields a shared 16-byte AES key, plus an out-of-band check against `https://sessionserver.mojang.com` that proves the joining client owns the Mojang account whose username it claims.

1. **Login Start.** The client sends [Hello (Login Start)](#0x00---hello-login-start) with the chosen username and the player's offline-or-cached UUID.
2. **Compression (optional).** If the server has a non-negative compression threshold configured, it sends [Login Compression](#0x03---login-compression-set-compression). All subsequent packets — in this state and every later state — are framed using the compressed framing described in the [Overview](./index#framing).
3. **Encryption Request.** In online mode, the server generates a random 4-byte verify token, picks (or reuses) a 1024-bit RSA keypair, and sends [Hello (Encryption Request)](#0x01---hello-encryption-request) containing its server-id string, the public key in X.509 SubjectPublicKeyInfo encoding, the verify token, and the `shouldAuthenticate` flag.
4. **Encryption Response.** The client generates a random 16-byte AES secret, encrypts both the secret and the verify token with the server's RSA public key, and replies with [Key (Encryption Response)](#0x01---key-encryption-response). On receiving it the server decrypts both fields, verifies the token matches the one it sent, and from that point on encrypts every byte of the framed stream with AES/CFB8 keyed by the shared secret. The client switches its own cipher state symmetrically.
5. **Mojang session check.** If `shouldAuthenticate` was `true`, the server now computes a SHA-1 digest over `serverId || sharedSecret || serverPublicKeyBytes` (concatenated raw bytes), formats the digest as a *signed* big-integer two's-complement hexadecimal string (the famous "Notchian hash" — leading minus sign for negative digests, no zero-padding), and queries `https://sessionserver.mojang.com/session/minecraft/hasJoined?username=<name>&serverId=<hash>`. A `200 OK` response carries the authoritative `GameProfile` (UUID + skin/cape properties); anything else means the client failed authentication and must be disconnected.
6. **Login Plugin Query (optional).** Either before or after authentication the server may send any number of [Custom Query](#0x04---custom-query-login-plugin-request) packets. Each carries a transaction ID and an opaque payload identified by a namespaced ID; the client replies with a matching [Custom Query Answer](#0x02---custom-query-answer-login-plugin-response). Used by Velocity/BungeeCord modern forwarding.
7. **Login Success.** With a profile in hand the server sends [Game Profile (Login Success)](#0x02---game-profile-login-success) carrying the full `GameProfile`.
8. **Login Acknowledged.** The client confirms receipt with [Login Acknowledged](#0x03---login-acknowledged); both sides switch to [Configuration](./configuration).

If at any step the server needs to abort, it sends [Disconnect (Login)](#0x00---disconnect-login) with a [Text Component](./text-component) describing the failure and closes the socket. Cookies (see [Store Cookie](./configuration#store-cookie)) may be requested in the Login state with [Cookie Request](#0x05---cookie-request); this is most useful immediately after a [Transfer](./configuration#transfer)-initiated reconnect.

---

## Client-bound

### 0x00 - Disconnect (Login)

**Packet ID:** `0x00` · **State:** Login · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Reason | JSON [Text Component](./text-component) | The disconnect message shown on the client's "Disconnected" screen. Encoded as a JSON string (not as the binary NBT component used in Configuration/Play) because the client may not yet have a registry context to decode NBT. |

**Semantics.** Server-initiated termination of the Login state. The connection is closed immediately after sending. Equivalent in role to the Configuration/Play `Disconnect` but with a JSON-string payload.

### 0x01 - Hello (Encryption Request)

**Packet ID:** `0x01` · **State:** Login · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Server ID | [String](./data-types#string) (20) | A short ASCII identifier salted into the Mojang `hasJoined` hash. Vanilla servers send the empty string. |
| Public Key | [Byte Array](./data-types#byte-array) ([VarInt](./data-types#varint)-prefixed) | The server's RSA-1024 public key, in X.509 `SubjectPublicKeyInfo` DER form. |
| Verify Token | [Byte Array](./data-types#byte-array) ([VarInt](./data-types#varint)-prefixed) | A 4-byte random nonce. The client must echo it back (encrypted) in [Key](#0x01---key-encryption-response) so the server can prove the response wasn't replayed. |
| Should Authenticate | [Boolean](./data-types#boolean) | When `true`, the server will perform the Mojang session check after decrypting Key. When `false`, the server skips the session call (used by velocity-style forwarding). |

**Semantics.** Sent after [Hello (Login Start)](#0x00---hello-login-start) when the server is in online mode. After this packet the next expected serverbound packet is [Key](#0x01---key-encryption-response).

### 0x02 - Game Profile (Login Success)

**Packet ID:** `0x02` · **State:** Login · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Game Profile | GameProfile | The authenticated profile: UUID (16 bytes), name (String, 16), and an array of `(name, value, optional signature)` properties. The `textures` property carries the player's skin/cape. See [Data Types](./data-types#gameprofile). |

**Semantics.** Terminal client-bound packet of the Login state. The server stops handling Login packets after sending it and waits for [Login Acknowledged](#0x03---login-acknowledged).

### 0x03 - Login Compression (Set Compression)

**Packet ID:** `0x03` · **State:** Login · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Threshold | [VarInt](./data-types#varint) | Minimum uncompressed packet size, in bytes, that triggers zlib compression. A value `< 0` disables compression entirely. |

**Semantics.** When sent, both sides immediately switch to the compressed framing described in the [Overview](./index#framing). Sent at most once per connection, and only before [Game Profile](#0x02---game-profile-login-success).

### 0x04 - Custom Query (Login Plugin Request)

**Packet ID:** `0x04` · **State:** Login · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Transaction ID | [VarInt](./data-types#varint) | Unique per outstanding query; the client must echo it in its [Custom Query Answer](#0x02---custom-query-answer-login-plugin-response). |
| Channel | [Identifier](./data-types#identifier) | Namespaced channel identifying the plugin protocol. |
| Data | [Byte Array](./data-types#byte-array) (no length prefix; consumes the rest of the packet) | Plugin-defined payload. Maximum 1 048 576 bytes. |

**Semantics.** Lets the server query the client for plugin-defined information before authentication completes. Most commonly used by proxies for modern player-info forwarding.

### 0x05 - Cookie Request

**Packet ID:** `0x05` · **State:** Login · **Bound To:** Client

| Field | Type | Notes |
|-------|------|-------|
| Key | [Identifier](./data-types#identifier) | The cookie key whose value the server wants. |

**Semantics.** Asks the client to return a previously-stored cookie (see [Store Cookie](./configuration#store-cookie)). The client must reply with [Cookie Response](#0x04---cookie-response). Useful immediately after a [Transfer](./configuration#transfer) so the new server can recover the previous server's session token.

---

## Server-bound

### 0x00 - Hello (Login Start)

**Packet ID:** `0x00` · **State:** Login · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Name | [String](./data-types#string) (16) | Player username chosen for this session. Servers in online mode treat this as a *claim* to be verified by the Mojang session check. |
| Player UUID | [UUID](./data-types#uuid) | The client-known UUID for this profile. In online mode it is overwritten by the authoritative UUID returned by Mojang; in offline mode the server typically derives the UUID from the username via `UUID.nameUUIDFromBytes("OfflinePlayer:<name>")`. |

**Semantics.** First packet of the Login state. Triggers the server to either send [Hello (Encryption Request)](#0x01---hello-encryption-request) (online mode) or skip straight to [Login Compression](#0x03---login-compression-set-compression) followed by [Game Profile](#0x02---game-profile-login-success) (offline mode).

### 0x01 - Key (Encryption Response)

**Packet ID:** `0x01` · **State:** Login · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Shared Secret | [Byte Array](./data-types#byte-array) ([VarInt](./data-types#varint)-prefixed) | The 16-byte AES shared secret, RSA-encrypted with the server's public key from [Hello (Encryption Request)](#0x01---hello-encryption-request). |
| Verify Token | [Byte Array](./data-types#byte-array) ([VarInt](./data-types#varint)-prefixed) | The verify token the server sent, RSA-encrypted with the same key. The server compares the decrypted plaintext byte-for-byte to detect MITM/replay. |

**Semantics.** Concludes the encryption handshake. After receiving and validating it the server keys its AES/CFB8 cipher and either runs the Mojang session check or, if `shouldAuthenticate` was `false`, proceeds straight to [Game Profile](#0x02---game-profile-login-success).

### 0x02 - Custom Query Answer (Login Plugin Response)

**Packet ID:** `0x02` · **State:** Login · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Transaction ID | [VarInt](./data-types#varint) | Echoes the ID from the corresponding [Custom Query](#0x04---custom-query-login-plugin-request). |
| Successful | [Boolean](./data-types#boolean) (encoded via `Optional` prefix) | `true` if the client understood the channel and provides a payload below; `false` otherwise (the Data field is absent in that case). |
| Data | [Byte Array](./data-types#byte-array) (consumes rest of packet) | Plugin-defined response payload, present only when Successful is `true`. Maximum 1 048 576 bytes. |

**Semantics.** Sent in reply to a Custom Query. Multiple may be in flight; the server matches them by Transaction ID.

### 0x03 - Login Acknowledged

**Packet ID:** `0x03` · **State:** Login · **Bound To:** Server

This packet has no fields.

**Semantics.** Terminal server-bound packet of the Login state. On receipt the server switches its connection state to [Configuration](./configuration); the client must do the same immediately after sending. Sent by the client in response to [Game Profile](#0x02---game-profile-login-success).

### 0x04 - Cookie Response

**Packet ID:** `0x04` · **State:** Login · **Bound To:** Server

| Field | Type | Notes |
|-------|------|-------|
| Key | [Identifier](./data-types#identifier) | Echoes the key from the [Cookie Request](#0x05---cookie-request). |
| Payload | Optional [Byte Array](./data-types#byte-array) ([VarInt](./data-types#varint)-prefixed, max 5120 bytes) | The stored cookie value, or absent if the client has no value for that key. |

**Semantics.** Sent in reply to [Cookie Request](#0x05---cookie-request). The payload is opaque to the client — it merely echoes whatever a server previously stored with [Store Cookie](./configuration#store-cookie).
