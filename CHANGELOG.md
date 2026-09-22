# Changelog

All notable changes to this project will be documented in this file.
## [0.1.1](https://github.com/VoidMinecraft/VoidMC/compare/voidmc-world-io-v0.1.0...voidmc-world-io-v0.1.1) - 2026-09-22

### Bug Fixes

- *(block-entity)* Column-checked chunk storage, strict host asset, exact metadata strip on load
- *(demo)* Bolt sway like the reference, width-only fade, port wording

### Features

- Expand flame profiling tooling
- *(logging)* Log player and world events
- *(biomes)* Cell-granular biome get/set, ChunksBiomes, generator hook and BiomeBuilder
- *(world)* Block entity storage, typed Sign/Skull/Banner, sync and persistence
- *(demo)* Alpine Rush — minecart racing demo on the engine APIs

## [0.3.0](https://github.com/VoidMinecraft/VoidMC/compare/voidmc-v0.2.0...voidmc-v0.3.0) - 2026-09-22

### Bug Fixes

- *(commands)* Expose summon position and flags
- *(status)* Advertise correct protocol version
- *(status)* Report online player count
- *(interaction)* Synchronize multiplayer block changes
- *(status)* Respond without waiting for tick
- *(network)* Bound packet framing and decoding
- *(network)* Make connection I/O cancellation-safe
- *(position)* Teleport instead of overflowing i16 movement delta
- *(entity)* Var-long metadata serializer, public MetadataSource registration, display clear()
- *(block-entity)* Column-checked chunk storage, strict host asset, exact metadata strip on load
- *(net)* Quiet disconnect races and detach after quit event

### Chores

- *(void)* Add keepalive test
- *(block-entity)* Trim plugin module doc

### Documentation

- *(position)* Document TeleportEntity fallback, drop manual changelog entry
- *(server)* Document observers, send API and VoidSystems; strip comments

### Features

- *(logging)* Log player and world events
- *(players)* Entity-addressed packet send API and migrate all call sites
- *(schedule)* Public VoidSystems system sets for framework phases
- *(boss-bar)* BossBar component, BossBarPlugin and Boss Event packet
- *(sounds)* Sound request API with Sound Effect, Entity Sound Effect and Stop Sound packets
- *(biomes)* Cell-granular biome get/set, ChunksBiomes, generator hook and BiomeBuilder
- *(particles)* Fire-and-forget particle requests with chunk-viewer audience
- *(entity)* Entity metadata, display entities and passengers
- *(world)* Block entity storage, typed Sign/Skull/Banner, sync and persistence
- *(entity)* Fluent EntityBuilder::with for pre-spawn components
- *(entity)* End Crystal metadata source

### Performance

- *(entity)* Chunk-indexed visibility tracker

### Refactor

- *(players)* Hoist ready() snapshots, derive From impls, cover all sets
- *(boss-bar)* Audience on the component, clamp on the wire, allocation-free steady state
- *(sounds)* Dimension on Sound, at(dimension, pos), play reports real delivery
- *(biomes)* Re-export Tag, tolerant palette reads, register-before-login guard
- *(particles)* Explicit send(), borrowing patch encode, registry-resolved position sources

### Style

- *(position)* Drop redundant comments
- *(position)* Trim comments

## [0.4.0](https://github.com/VoidMinecraft/VoidMC/compare/voidmc-protocol-v0.3.0...voidmc-protocol-v0.4.0) - 2026-09-22

### Bug Fixes

- *(commands)* Expose summon position and flags
- *(status)* Advertise correct protocol version
- *(interaction)* Synchronize multiplayer block changes
- *(network)* Bound packet framing and decoding
- *(entity)* Var-long metadata serializer, public MetadataSource registration, display clear()
- *(menu)* Full resync on title change, keep hidden inventory changes, creative-only clone, cursor return on disconnect, click packet hardening
- *(entity)* Passenger-aware player broadcast and settle snap delta
- *(entity)* Passenger-aware movement ingest, mount-guarded unmount and position sync resync

### Chores

- *(boss-bar)* Adapt to bounded Decoder and register BossEvent in the packet-id guard

### Documentation

- *(server)* Document observers, send API and VoidSystems; strip comments

### Features

- *(data)* Generate packet ids and version info; guard protocol against silent drift
- *(players)* Entity-addressed packet send API and migrate all call sites
- *(boss-bar)* BossBar component, BossBarPlugin and Boss Event packet
- *(sounds)* Sound request API with Sound Effect, Entity Sound Effect and Stop Sound packets
- *(biomes)* Cell-granular biome get/set, ChunksBiomes, generator hook and BiomeBuilder
- *(protocol)* LevelParticles packet with typed Particle enum
- *(entity)* Entity metadata, display entities and passengers
- *(protocol)* Block Entity Data packet and typed chunk block-entity entries
- *(protocol)* Open Screen, Set Cooldown, clientbound Close Container and a fully decoded Container Click
- *(entity)* End Crystal metadata source

### Refactor

- *(players)* Hoist ready() snapshots, derive From impls, cover all sets
- *(boss-bar)* Audience on the component, clamp on the wire, allocation-free steady state
- *(biomes)* Re-export Tag, tolerant palette reads, register-before-login guard
- *(particles)* Explicit send(), borrowing patch encode, registry-resolved position sources

### Tests

- *(biomes)* Missing palette longs read as palette entry 0

### Merge

- Feat/player-transfer into fix/passenger-broadcast-and-settle
- Feat/entity-hidden into feat/effects-attributes
- Feat/effects-attributes into feat/world-border
- Feat/world-border into feat/time-weather
- Feat/time-weather into feat/titles
- Feat/titles into feat/tab-list
- Feat/tab-list into feat/scoreboard
- Feat/sidebar into feat/toasts
- Fix/messages-typed-color into feat/toasts

## [0.2.0](https://github.com/VoidMinecraft/VoidMC/compare/voidmc-net-v0.1.1...voidmc-net-v0.2.0) - 2026-09-22

### Bug Fixes

- *(network)* Bound packet framing and decoding
- *(network)* Make connection I/O cancellation-safe
- *(demo)* Bolt sway like the reference, width-only fade, port wording

### Features

- Expand flame profiling tooling
- *(demo)* Alpine Rush — minecart racing demo on the engine APIs

### Performance

- *(net)* Coalesce outbound writes into buffered per-batch flush

## [0.1.3](https://github.com/VoidMinecraft/VoidMC/compare/voidmc-data-v0.1.2...voidmc-data-v0.1.3) - 2026-09-22

### Bug Fixes

- *(block-entity)* Column-checked chunk storage, strict host asset, exact metadata strip on load
- *(data)* Allow too_many_arguments on emit_blocks_module
- *(commands)* Apply review nits on argument parsers and completion

### Chores

- *(data)* Restore full registries.json, ship packets.json and version.json

### Features

- Expand flame profiling tooling
- *(data)* Generate packet ids and version info; guard protocol against silent drift
- *(biomes)* Cell-granular biome get/set, ChunksBiomes, generator hook and BiomeBuilder
- *(data)* Block entity host table and type name lookup
- *(commands)* Typed Brigadier argument parsers and client-side completion

### Refactor

- *(data)* Group emit_blocks_module inputs into a struct

## [0.2.0](https://github.com/VoidMinecraft/VoidMC/compare/voidmc-codec-v0.1.1...voidmc-codec-v0.2.0) - 2026-09-22

### Bug Fixes

- *(network)* Bound packet framing and decoding
- *(demo)* Bolt sway like the reference, width-only fade, port wording

### Features

- Expand flame profiling tooling
- *(demo)* Alpine Rush — minecart racing demo on the engine APIs

### Merge

- Feat/toasts into feat/demo

## [0.1.2](https://github.com/VoidMinecraft/VoidMC/compare/voidmc-codec-macros-v0.1.1...voidmc-codec-macros-v0.1.2) - 2026-09-22

### Bug Fixes

- *(network)* Bound packet framing and decoding
- *(demo)* Bolt sway like the reference, width-only fade, port wording

### Features

- Expand flame profiling tooling
- *(demo)* Alpine Rush — minecart racing demo on the engine APIs

### Refactor

- *(players)* Hoist ready() snapshots, derive From impls, cover all sets

### Merge

- Feat/toasts into feat/demo

## [0.1.0](https://github.com/VoidMinecraft/VoidMC/releases/tag/voidmc-world-io-v0.1.0) - 2026-06-25

### Documentation

- Added root README.md
- Add architecture readiness evidence

### Features

- World serialization (F8) and graceful /stop command

### Refactor

- Rename crates to voidmc-* prefix

## [0.2.0](https://github.com/VoidMinecraft/VoidMC/compare/voidmc-v0.1.0...voidmc-v0.2.0) - 2026-06-25

### Bug Fixes

- *(net)* Log unrecognized packets as warning instead of error

### Features

- World serialization (F8) and graceful /stop command

## [0.3.0](https://github.com/VoidMinecraft/VoidMC/compare/voidmc-protocol-v0.2.0...voidmc-protocol-v0.3.0) - 2026-06-25

## [0.1.1](https://github.com/VoidMinecraft/VoidMC/compare/voidmc-net-v0.1.0...voidmc-net-v0.1.1) - 2026-06-25

### Bug Fixes

- *(net)* Log unrecognized packets as warning instead of error

## [0.1.2](https://github.com/VoidMinecraft/VoidMC/compare/voidmc-data-v0.1.1...voidmc-data-v0.1.2) - 2026-06-25

## [0.1.1](https://github.com/VoidMinecraft/VoidMC/compare/voidmc-codec-v0.1.0...voidmc-codec-v0.1.1) - 2026-06-25

### Documentation

- Add tech watch evidence
- Add architecture readiness evidence

## [0.1.1](https://github.com/VoidMinecraft/VoidMC/compare/voidmc-codec-macros-v0.1.0...voidmc-codec-macros-v0.1.1) - 2026-06-25

### Documentation

- Add architecture readiness evidence

## [0.1.0](https://github.com/VoidMinecraft/VoidMC/releases/tag/voidmc-v0.1.0) - 2026-06-24

### Bug Fixes

- Removed dead code, added missing test attributes and passing NBT as ref when encoding
- *(void,void-net)* Login packet last death location field
- *(void)* Register player ready/quit observers so players see each other
- *(void)* Add missing vanilla damage types to registry
- Resolve rustfmt and clippy warnings
- *(protocol)* Encode add_entity velocity as LP Vec3 (1.21.7+)
- *(tests)* Fixed tests.

### Chores

- Removed game mod
- Renamed ServerBuilder to ServerConfigBuilder
- Formatted files
- Improved example
- *(workspace)* Prepare crates for crates.io publish
- *(entities-ai)* Sync with summon lifecycle

### Documentation

- Added root README.md

### Features

- Setup void crate
- *(void)* Added game shared struct
- *(void)* Added client and handshake client
- *(void)* Added server and server builder
- *(void)* Added status client
- *(void)* Added base of login client
- *(void, void-net)* Finish login & login acknowledgement logics
- *(void)* Added base of configuration client
- *(void,configuration)* Sending known packs, registries data and finish configuration packets
- *(void)* Added base of play client
- *(void-net)* Added clientboundpackets for the Play state
- *(void-net)* Added serverbound packets for the Play state
- *(void)* Sending Login, GameEvent and SynchronizePlayerPosition packets when switching to Play state
- *(void)* Added some logs message on important event
- Adapted void-net and void crates with new void-protocol crate
- *(void-protocol,void)* Improved packet typings
- *(void-protocol,void)* Added ConfirmTeleportation packet
- Improved error messages
- *(void-protocol,void)* Added plugin message packet
- *(void-protocol,void)* Added player_loaded and set_player_pos_and_rot packets
- *(void)* Added tracing logging
- Sending packets to the correct clients
- Improved client_id generation
- Add ECS components, events, and resources for player data
- Add handler systems for all protocol states (handshake, status, login, configuration, play)
- Add game systems for keep-alive and position broadcasting
- Wire plugins, registry data, and module declarations for full join flow
- Add clientbound packets for player visibility and cleanup
- Add serverbound SetPlayerRotation packet and handler
- Add disconnect notification channel
- Add player spawn/disconnect systems and rotation broadcasting
- Add world module with chunk positions, dimensions, and terrain generation
- Add command system with typed argument parsing and kick tab-completion
- Add /tp command with typed double arguments
- Add /broadcast command
- Add /tell command with /msg alias for private messaging
- Add /list command to show online players
- Add /say command
- Add command suggestions (tab-completion) for player names
- Integrate command system with chat handling and kick pipeline
- Add 9 serverbound packets and 14 game events for plugin developers
- *(void)* Converted handshake and status packets handling into plugins
- *(void)* Converted login packets handling into plugin
- *(void)* Converted configuration packets handling into plugin
- *(void)* Converted play packets related to movement into a plugin
- *(void)* Converted base packets handling from play state to a plugin
- *(void)* Converted play state handling about chat into a plugin
- *(void)* Converted play packet handling related to interaction into a plugin
- *(void)* Adapted commands
- *(void)* Wire void-data registry into the server
- *(void)* Adapt status, login and configuration plugins to 26.1.2
- *(void)* Adapt chat plugin to 26.1.2 signed/unsigned chat split
- *(void)* Adapt chunk entity and generation systems for 26.1.2 chunk format
- Added TPS and flamechart metrics
- Optimized chunk streaming and packet ingestion
- *(interactions)* Authoritative world mutation, typed block data
- *(summon)* Add non-player entity lifecycle

### Refactor

- *(void)* Extracted clients per state
- Splitted ingest_network_packets method and moved client metadata to components
- Extracted network stuff into a plugin
- Splitted handle functions
- Rewrite network.rs to use event dispatch with immediate state transitions
- Extract server config, registry, and app entry point into library
- Switch to event-driven handler dispatch with chunk streaming
- Event-driven packet handling with per-state plugins
- Rename crates to voidmc-* prefix
- *(example)* Move circle command to void-example

### Style

- Cargo fmt + collapse packet_debug if-let chain

## [0.2.0](https://github.com/VoidMinecraft/VoidMC/compare/voidmc-protocol-v0.1.0...voidmc-protocol-v0.2.0) - 2026-06-24

### Bug Fixes

- *(protocol)* Encode add_entity velocity as LP Vec3 (1.21.7+)
- *(tests)* Fixed tests.
- *(protocol)* Correct resource location parser id

### Features

- *(interactions)* Authoritative world mutation, typed block data
- *(protocol)* Add entity motion sync packets

## [0.1.1](https://github.com/VoidMinecraft/VoidMC/compare/voidmc-data-v0.1.0...voidmc-data-v0.1.1) - 2026-06-24

### Bug Fixes

- *(void-data)* Emit is_multiple_of in block codegen for clippy 1.95

### Chores

- *(entities-ai)* Sync with summon lifecycle

### Features

- *(interactions)* Authoritative world mutation, typed block data
- *(void-data)* Generate entity type protocol data

### Style

- *(void-data)* Rustfmt build.rs codegen line

## [0.1.0](https://github.com/VoidMinecraft/VoidMC/releases/tag/voidmc-codec-macros-v0.1.0) - 2026-05-03

### Bug Fixes

- Resolve rustfmt and clippy warnings

### Chores

- Formatted files
- *(workspace)* Prepare crates for crates.io publish

### Documentation

- Added root README.md

### Features

- Added void-codec and void-codec-macro crates
- *(void-codec)* Added u16, String and json
- Improved error messages
- *(void-codec)* Added fixed length array and remaining

### Refactor

- Rename crates to voidmc-* prefix
