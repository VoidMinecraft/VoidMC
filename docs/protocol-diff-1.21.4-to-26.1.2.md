# Protocol diff — 1.21.4 → 26.1.2

> Phase 0 deliverable. Source of truth: vanilla `*Protocols.java` files extracted
> from `papermc/paper-server/src/minecraft/java/net/minecraft/network/protocol/`
> after `./gradlew applyPatches` (Paper at `mcVersion=26.1.2`).
>
> Vanilla packet IDs are the **insertion order** in
> `ProtocolInfoBuilder.addPacket(...)` chains (confirmed by reading
> `ProtocolInfoBuilder.listPackets`, line 80-81, `output.accept(entry.type, i)`).
>
> Numbering for clientbound Play starts at **1** because `withBundlePacket`
> registers `ClientboundBundleDelimiterPacket` at ID 0.

## Protocol version

| Marketing | Protocol | Notes |
|-----------|----------|-------|
| 1.21.4 | 769 | current target of `void/` |
| 26.1.2 | 773 | new target |

Status response must advertise `protocol_version = 773`, `version_name = "26.1.2"`.

## Phase summary

| Phase | Verdict |
|-------|---------|
| Handshake | **No changes**. `ClientIntentionPacket` shape unchanged: `(varint protocol, string hostname, ushort port, varint intent)`. |
| Status | **No structural changes**. Only update `protocol_version` and `version_name` in the JSON. |
| Login | Packet IDs unchanged. **`LoginSuccess` lost `strict_error_handling`** (1.21.5+). Otherwise stable. |
| Configuration | Packet IDs **unchanged for the packets we use**. New optional packets (dialogs, code of conduct). |
| Play | **All packet IDs shifted** because of insertions in 1.21.5/1.21.6/1.21.10. Many packets renamed. Few format changes. |

---

## Login phase

### Clientbound (`LoginProtocols.java`)

Packet IDs in 26.1.2:

| Vanilla name | ID | Our name | Status |
|--------------|----|----------|--------|
| `LoginDisconnect` | 0 | — | not implemented (ok) |
| `Hello` (encryption request) | 1 | — | not implemented (ok) |
| `LoginFinished` | 2 | `LoginSuccess` | **id unchanged**, drop `strict_error_handling` field |
| `LoginCompression` | 3 | — | not implemented (ok) |
| `CustomQuery` | 4 | — | not implemented (ok) |
| `CookieRequest` | 5 | — | not implemented (ok) |

**`LoginSuccess` (now `LoginFinished`)**: in 26.1.2 the codec is just `GameProfile` (uuid + name + properties). Our current code likely writes a trailing boolean (`strict_error_handling`) that must be removed.

### Serverbound

| Vanilla name | ID | Our name | Status |
|--------------|----|----------|--------|
| `Hello` | 0 | `LoginStart` | unchanged. Fields: `(string name, uuid profile_id)` |
| `Key` | 1 | — | not implemented |
| `CustomQueryAnswer` | 2 | — | not implemented |
| `LoginAcknowledged` | 3 | `LoginAcknowledged` | unchanged |
| `CookieResponse` | 4 | — | not implemented |

✅ No serverbound changes for the packets we handle.

---

## Configuration phase

### Clientbound (`ConfigurationProtocols.java`)

| ID | Vanilla name | Our name | Status |
|----|--------------|----------|--------|
| 0 | `CookieRequest` | — | not implemented |
| 1 | `CustomPayload` | — | not implemented |
| 2 | `Disconnect` | — | not implemented |
| 3 | `FinishConfiguration` | `FinishConfiguration` | ✓ unchanged |
| 4 | `KeepAlive` | — | not implemented (config-phase keepalive) |
| 5 | `Ping` | — | not implemented |
| 6 | `ResetChat` | — | NEW, not needed |
| 7 | `RegistryData` | `RegistryData` | ✓ unchanged |
| 8 | `ResourcePackPop` | — | not implemented |
| 9 | `ResourcePackPush` | — | not implemented |
| 10 | `StoreCookie` | — | not implemented |
| 11 | `Transfer` | — | not implemented |
| 12 | `UpdateEnabledFeatures` | — | not implemented |
| 13 | `UpdateTags` | `UpdateTags` (manual) | ✓ unchanged |
| 14 | `SelectKnownPacks` | `KnownPacks` | ✓ unchanged (id 0x0E) |
| 15 | `CustomReportDetails` | — | not implemented |
| 16 | `ServerLinks` | — | not implemented |
| 17 | `ClearDialog` | — | NEW (1.21.6) |
| 18 | `ShowDialog` | — | NEW (1.21.6) |
| 19 | `CodeOfConduct` | — | NEW (26.1.x) |

✅ **All clientbound configuration packets we use keep the same IDs.**

### Serverbound

| ID | Vanilla name | Our name | Status |
|----|--------------|----------|--------|
| 0 | `ClientInformation` | `ClientInformation` | ✓ unchanged |
| 1 | `CookieResponse` | — | not implemented |
| 2 | `CustomPayload` | `PluginMessage` | ✓ unchanged |
| 3 | `FinishConfiguration` | `FinishConfigurationAcknowledged` | ✓ unchanged |
| 4 | `KeepAlive` | — | not implemented |
| 5 | `Pong` | — | not implemented |
| 6 | `ResourcePack` | — | not implemented |
| 7 | `SelectKnownPacks` | `KnownPacks` | ✓ unchanged |
| 8 | `CustomClickAction` | — | NEW |
| 9 | `AcceptCodeOfConduct` | — | NEW (26.1.x) |

✅ Serverbound configuration we handle keeps IDs.

---

## Play phase

### Clientbound (`GameProtocols.java`)

ID 0 is the `BundleDelimiter`. Real packets start at 1.

Mapping for packets we currently emit (1.21.4 ID → 26.1.2 ID):

| Our name | 1.21.4 ID | Vanilla name | 26.1.2 ID | Field changes |
|----------|-----------|--------------|-----------|---------------|
| `SpawnEntity` | 0x01 | `AddEntity` | **0x01** | none |
| `Disconnect` | 0x1D | `Disconnect` | **0x20** (32) | none |
| `UnloadChunk` | 0x22 | `ForgetLevelChunk` | **0x25** (37) | none |
| `GameEvent` | 0x23 | `GameEvent` | **0x26** (38) | none |
| `KeepAlive` | 0x27 | `KeepAlive` | **0x2C** (44) | none |
| `Login` | 0x2C | `Login` | **0x31** (49) | **shape unchanged** ¹ |
| `UpdateEntityPosition` | 0x2F | `MoveEntityPos` | **0x35** (53) | none |
| `UpdateEntityPositionAndRotation` | 0x30 | `MoveEntityPosRot` | **0x36** (54) | none |
| `UpdateEntityRotation` | 0x32 | `MoveEntityRot` | **0x38** (56) | none |
| `Ping` | 0x37 | `Ping` | **0x3D** (61) | none |
| `SynchronizePlayerPosition` | 0x42 | `PlayerPosition` | **0x48** (72) | renamed; format unchanged ² |
| `SetHeadRotation` | 0x4D | `RotateHead` | **0x53** (83) | none |
| `SetCenterChunk` | 0x58 | `SetChunkCacheCenter` | **0x5E** (94) | none |
| `SystemChat` | 0x73 | `SystemChat` | **0x79** (121) | none |
| `PlayerInfoUpdate` (manual) | 0x40 | `PlayerInfoUpdate` | **0x46** (70) | **check display-name flag** ³ |
| `PlayerInfoRemove` (manual) | 0x3F | `PlayerInfoRemove` | **0x45** (69) | none |
| `RemoveEntities` (manual) | 0x47 | `RemoveEntities` | **0x4D** (77) | none |
| `ChunkDataAndLight` (manual) | 0x28 | `LevelChunkWithLight` | **0x2D** (45) | none |
| `Commands` (manual) | 0x11 | `Commands` | **0x10** (16) | check node format ⁴ |
| `CommandSuggestionsResponse` (manual) | 0x10 | `CommandSuggestions` | **0x0F** (15) | none |

¹ `Login` (Play): same record fields as 1.21.4 — confirmed by reading
  `ClientboundLoginPacket.java` and `CommonPlayerSpawnInfo.java` in
  `papermc/paper-server/src/minecraft/java/.../game/`. The 1.21.5 *login phase*
  `LoginFinished` is the one that lost `strict_error_handling`, not the play
  `Login` packet.

² `SynchronizePlayerPosition` was renamed to `PlayerPosition` in 1.21.5 and
  the structure was refactored into `PositionMoveRotation`, but the wire
  format stays: `varint teleport_id`, then `(double x, double y, double z,
  double dx, double dy, double dz, float yaw, float pitch)`, then
  `int relative_flags`. **Worth re-validating once.**

³ `PlayerInfoUpdate`: in 1.21.6 a new action bit `UPDATE_LIST_PRIORITY` was
  added (bit 0x40) and `UPDATE_HAT` (0x80) in 26.1.x. We currently send
  `ADD_PLAYER + UPDATE_LISTED + UPDATE_GAME_MODE` only — these bits are stable.
  No code change required unless new actions are wanted.

⁴ `Commands` packet: command node serialization for argument parsers is
  versioned and grows over time. New parsers may appear. Validate with a
  simple tab-completion test.

### New clientbound play packets to be aware of (26.1.2)

These are insertions that caused the ID shifts. None are mandatory for our
current scope (they're not sent until you trigger their feature):

- `ChunkBatchFinished` / `ChunkBatchStart` (already in 1.21.4)
- `ChunksBiomes` (already in 1.21.4)
- `DamageEvent` (1.21+)
- `DebugBlockValue` / `DebugChunkValue` / `DebugEntityValue` / `DebugSample` (debug)
- `MoveMinecartAlongTrack` (1.21.6)
- `LowDiskSpaceWarning` (26.1.x)
- `RecipeBookAdd` / `RecipeBookRemove` / `RecipeBookSettings` (1.21.5 redesigned recipe book)
- `SetCursorItem` (1.21.5 inventory rework)
- `SetPlayerInventory` (1.21.5)
- `TestInstanceBlockStatus` (1.21.6)
- `Waypoint` (1.21.6 tracked waypoints)
- `ClearDialog` / `ShowDialog` (1.21.6)
- `ProjectilePower` (1.21+)

### Serverbound (`GameProtocols.java`)

Mapping for packets we currently handle:

| Our name | 1.21.4 ID | Vanilla name | 26.1.2 ID | Notes |
|----------|-----------|--------------|-----------|-------|
| `ConfirmTeleportation` | 0x00 | `AcceptTeleportation` | **0x00** | unchanged |
| `ChatCommand` | 0x04 | `ChatCommand` | **0x07** | shifted |
| `ChatCommandUnsigned` | 0x05 | — | — | **probable bug** in our code: vanilla doesn't have a separate "unsigned" variant; we should keep just `ChatCommand` |
| `SignedChatCommand` | 0x06 | `ChatCommandSigned` | **0x08** | renamed + shifted |
| `ChatMessage` | 0x07 | `Chat` | **0x09** | shifted |
| `TickEnd` | 0x0B | `ClientTickEnd` | **0x0D** | renamed + shifted |
| `ClientInformation` | 0x0C | `ClientInformation` (common) | **0x0E** | shifted |
| `CommandSuggestionsRequest` | 0x0D | `CommandSuggestion` | **0x0F** | shifted |
| `CloseContainer` | 0x11 | `ContainerClose` | **0x13** | shifted |
| `Interact` | 0x18 | `Interact` | **0x1A** | shifted |
| `KeepAlive` | 0x1A | `KeepAlive` (common) | **0x1C** | shifted |
| `SetPlayerPos` | 0x1C | `MovePlayerPos` | **0x1E** | shifted |
| `SetPlayerPosAndRot` | 0x1D | `MovePlayerPosRot` | **0x1F** | shifted |
| `SetPlayerRotation` | 0x1E | `MovePlayerRot` | **0x20** | shifted |
| `PlayerAbilities` | 0x26 | `PlayerAbilities` | **0x28** | shifted |
| `PlayerAction` | 0x27 | `PlayerAction` | **0x29** | shifted |
| `PlayerCommand` | 0x28 | `PlayerCommand` | **0x2A** | shifted |
| `Pong` | 0x29 | `Pong` (common) | **0x2D** | shifted |
| `PlayerLoaded` | 0x2A | `PlayerLoaded` | **0x2C** | shifted |
| `SetHeldItem` | 0x33 | `SetCarriedItem` | **0x35** | renamed + shifted |
| `SwingArm` | 0x3A | `Swing` | **0x3F** | renamed + shifted |
| `UseItemOn` | 0x3C | `UseItemOn` | **0x42** | shifted |
| `UseItem` | 0x3D | `UseItem` | **0x43** | shifted |

### Format changes (serverbound)

To **verify case by case** during implementation; based on a quick read, no
break for the packets we handle, but candidates to double-check:

- `MovePlayerPos` / `MovePlayerPosRot` / `MovePlayerRot` / `MovePlayerStatusOnly` —
  refactored into a parent `ServerboundMovePlayerPacket` family. Wire format
  should still be: `(double x, double y, double z, [float yaw, pitch], byte flags)`.
- `Interact` — added a `sneaking` boolean in 1.21.x already; should be unchanged.
- `PlayerCommand` — actions enum may have grown but our handler matches by
  variant name.

---

## Registries / data assets

`void-data/assets/` must be re-extracted from a Paper 26.1.2 jar via
`scripts/extract.sh 26.1.2 <paper-jar-url>`. New datapack-extractable
registries available from 1.21.5+ that we currently hardcode:

- `minecraft:cat_variant`
- `minecraft:chicken_variant`
- `minecraft:cow_variant`
- `minecraft:frog_variant`
- `minecraft:pig_variant`
- `minecraft:wolf_sound_variant`

These should be moved from `builtin_variant_registries()` (in
`void/src/registry.rs`) to the extracted set.

Likely **new** registries shipped by 26.1.2 (verify after extraction):

- `minecraft:dialog` (mandatory? probably empty-allowed)
- `minecraft:test_environment`, `minecraft:test_instance` (game-test only,
  likely safe to omit if `enabled_features` doesn't include the test pack)
- `minecraft:enchantment_provider` (probably already extractable)

## Action items for implementation

1. **Status response**: bump `protocol_version` to 773, `version_name` to
   `"26.1.2"` in `void/src/plugins/status.rs`.
2. **Login (login phase) packet 0x02**: drop `strict_error_handling` field if
   present.
3. **Play packet IDs**: update every ID in
   `void-protocol/src/clientbound/play.rs` and `serverbound/play.rs`,
   `clientbound/play.rs` `ManualPlayPacket::encode` block, and
   `void-protocol/src/clientbound/configuration.rs`
   `ManualConfigurationPacket::encode` (no change there but worth verifying).
4. **Cleanup**: remove the suspicious `ChatCommandUnsigned` enum variant if it
   doesn't correspond to a vanilla packet.
5. **`void-data` re-extraction**: run extract.sh against Paper 26.1.2;
   remove the 6 hardcoded variant registries from
   `void/src/registry.rs::builtin_variant_registries()`; add `Version::V26_1_2`
   in `void-data/src/lib.rs`.
6. **Tests + smoke test** with a 26.1.2 client.
