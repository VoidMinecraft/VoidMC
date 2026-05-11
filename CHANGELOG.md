# Changelog

All notable changes to this project will be documented in this file.
## [0.1.0](https://github.com/VoidMinecraft/VoidMC/releases/tag/voidmc-v0.1.0) - 2026-05-11

### Bug Fixes

- Removed dead code, added missing test attributes and passing NBT as ref when encoding
- *(void,void-net)* Login packet last death location field
- *(void)* Register player ready/quit observers so players see each other
- *(void)* Add missing vanilla damage types to registry
- Resolve rustfmt and clippy warnings
- *(protocol)* Encode add_entity velocity as LP Vec3 (1.21.7+)

### Chores

- Removed game mod
- Renamed ServerBuilder to ServerConfigBuilder
- Formatted files
- Improved example
- *(workspace)* Prepare crates for crates.io publish

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

### Style

- Cargo fmt + collapse packet_debug if-let chain

## [0.2.0](https://github.com/VoidMinecraft/VoidMC/compare/voidmc-protocol-v0.1.0...voidmc-protocol-v0.2.0) - 2026-05-11

### Bug Fixes

- *(protocol)* Encode add_entity velocity as LP Vec3 (1.21.7+)

### Features

- *(interactions)* Authoritative world mutation, typed block data

## [0.1.1](https://github.com/VoidMinecraft/VoidMC/compare/voidmc-data-v0.1.0...voidmc-data-v0.1.1) - 2026-05-11

### Bug Fixes

- *(void-data)* Emit is_multiple_of in block codegen for clippy 1.95

### Features

- *(interactions)* Authoritative world mutation, typed block data

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
