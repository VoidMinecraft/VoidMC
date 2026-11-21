# Changelog

All notable changes to this project will be documented in this file.
## [0.1.0](https://github.com/VoidMinecraft/VoidMC/releases/tag/voidmc-worldedit-v0.1.0) - 2026-11-21

### Bug Fixes

- *(demo)* Bolt sway like the reference, width-only fade, port wording
- *(worldedit)* Move schematic IO and clipboard transforms off the tick, bound NBT lengths and history entries
- *(worldedit)* Bound schematic loads by in-memory NBT size, cap background tasks, save atomically
- *(worldedit)* Bound schematic loading memory to one shared budget
- *(server)* Return the AppExit from VoidServer::run ([#179](https://github.com/VoidMinecraft/VoidMC/pull/179)) [**breaking**]
- *(worldedit)* Treat a time per tick past the clock's range as no limit

### Documentation

- Added root README.md
- Add architecture readiness evidence
- *(observability)* Document the health states and metric set; enable them in the example with VOID_OBSERVABILITY_ADDR
- Document TPS and flame output defaults, formats and failures
- Operator-only default commands, permission level and per-player tree

### Features

- Expand flame profiling tooling
- *(demo)* Alpine Rush — minecart racing demo on the engine APIs
- *(worldedit)* World editing crate with engine-agnostic core and VoidMC adapter
- *(example)* Make every player an operator by default

### Refactor

- Rename crates to voidmc-* prefix

### Tests

- *(worldedit)* Hold the wand in the used hand in the off-hand permission test
- *(worldedit)* Drop the wall-clock tick budget from the engine tests
- *(worldedit)* Click a stone, not air, to place the off-hand wand block

## [0.2.0](https://github.com/VoidMinecraft/VoidMC/compare/voidmc-world-io-v0.1.0...voidmc-world-io-v0.2.0) - 2026-11-21

### Bug Fixes

- *(block-entity)* Column-checked chunk storage, strict host asset, exact metadata strip on load
- *(demo)* Bolt sway like the reference, width-only fade, port wording
- *(server)* Stop gracefully on SIGTERM and SIGHUP, not only Ctrl-C
- *(server)* Return the AppExit from VoidServer::run ([#179](https://github.com/VoidMinecraft/VoidMC/pull/179)) [**breaking**]
- *(world-io)* Quarantine a stored chunk that cannot be loaded ([#189](https://github.com/VoidMinecraft/VoidMC/pull/189))
- *(world-io)* Run the periodic save after the block-entity sync

### Documentation

- *(observability)* Document the health states and metric set; enable them in the example with VOID_OBSERVABILITY_ADDR
- Document TPS and flame output defaults, formats and failures
- Operator-only default commands, permission level and per-player tree

### Features

- Expand flame profiling tooling
- *(logging)* Log player and world events
- *(biomes)* Cell-granular biome get/set, ChunksBiomes, generator hook and BiomeBuilder
- *(world)* Block entity storage, typed Sign/Skull/Banner, sync and persistence
- *(demo)* Alpine Rush — minecart racing demo on the engine APIs
- *(example)* Make every player an operator by default

### Tests

- Reproduce #189, a damaged stored chunk is regenerated and later overwritten
- *(world-io)* Save a block-entity change made in the tick of a periodic save

## [0.1.0](https://github.com/VoidMinecraft/VoidMC/releases/tag/voidmc-vanilla-commands-v0.1.0) - 2026-11-21

### Bug Fixes

- *(demo)* Bolt sway like the reference, width-only fade, port wording
- *(vanilla-commands)* Bound component nesting, warn on claimed display slots, keep foreign team entities
- *(vanilla-commands)* Only touch scores when a trigger is actually enabled
- *(vanilla-commands)* Check access before parsing, add online members by entity, resolve player UUIDs
- *(vanilla-commands)* Restore team members by entity when players log in
- *(server)* Return the AppExit from VoidServer::run ([#179](https://github.com/VoidMinecraft/VoidMC/pull/179)) [**breaking**]

### Documentation

- Added root README.md
- Add architecture readiness evidence
- *(observability)* Document the health states and metric set; enable them in the example with VOID_OBSERVABILITY_ADDR
- Document TPS and flame output defaults, formats and failures
- Operator-only default commands, permission level and per-player tree

### Features

- Expand flame profiling tooling
- *(demo)* Alpine Rush — minecart racing demo on the engine APIs
- *(vanilla-commands)* Vanilla /team and /scoreboard as an opt-in plugin crate
- *(example)* Make every player an operator by default

### Refactor

- Rename crates to voidmc-* prefix

## [0.1.0](https://github.com/VoidMinecraft/VoidMC/releases/tag/voidmc-navigation-v0.1.0) - 2026-11-21

### Bug Fixes

- *(demo)* Bolt sway like the reference, width-only fade, port wording
- *(navigation)* Hold single-point patrols and lower the default budget
- *(navigation)* Address review findings
- *(navigation)* Second review round
- *(navigation)* Fair long searches and exact cache invalidation
- *(navigation)* Serve cache hits to parked long searches
- *(navigation)* Clamp block coordinates so absurd goals cannot overflow
- *(server)* Return the AppExit from VoidServer::run ([#179](https://github.com/VoidMinecraft/VoidMC/pull/179)) [**breaking**]

### Documentation

- Added root README.md
- Add architecture readiness evidence
- Navigation reference page
- *(observability)* Document the health states and metric set; enable them in the example with VOID_OBSERVABILITY_ADDR
- Document TPS and flame output defaults, formats and failures
- Operator-only default commands, permission level and per-player tree

### Features

- Expand flame profiling tooling
- *(demo)* Alpine Rush — minecart racing demo on the engine APIs
- *(navigation)* Engine-agnostic voxel pathfinding core
- *(navigation)* VoidMC adapter with navigators, goals and behaviours
- *(navigation)* Operator remote to watch and drive navigators
- *(navigation)* Walking_profile helper and a full-stack remote test
- *(example)* Make every player an operator by default

### Refactor

- Rename crates to voidmc-* prefix

## [0.3.0](https://github.com/VoidMinecraft/VoidMC/compare/voidmc-v0.2.0...voidmc-v0.3.0) - 2026-11-21

### Bug Fixes

- Shut down network channels gracefully
- *(commands)* Expose summon position and flags
- *(entities)* Stabilize AI collision physics
- *(status)* Advertise correct protocol version
- *(status)* Report online player count
- *(interaction)* Synchronize multiplayer block changes
- *(movement)* Synchronize negative player rotations
- *(status)* Respond without waiting for tick
- *(network)* Bound packet framing and decoding
- *(network)* Make connection I/O cancellation-safe
- *(position)* Teleport instead of overflowing i16 movement delta
- *(entity)* Var-long metadata serializer, public MetadataSource registration, display clear()
- *(block-entity)* Column-checked chunk storage, strict host asset, exact metadata strip on load
- *(menu)* Full resync on title change, keep hidden inventory changes, creative-only clone, cursor return on disconnect, click packet hardening
- *(entity)* Require IndexedChunks on PlayerReady to avoid ghost viewers
- *(teleport)* Never deliver PlayerTeleportEvent for a despawned entity, drain chunk backlog without recompute
- *(net)* Quiet disconnect races and detach after quit event
- *(messages)* Typed TextColor, no client-kicking colours
- *(messages)* Truncate text on its modified-UTF-8 width
- *(text)* Guard every text component against oversized modified UTF-8
- *(world)* Guard sign lines against oversized modified UTF-8
- *(entity)* Compute relative movement deltas in fixed point like VecDeltaCodec
- *(entity)* Passenger-aware player broadcast and settle snap delta
- *(entity)* Passenger-aware movement ingest, mount-guarded unmount and position sync resync
- *(titles)* Apply review nits on titles API
- *(movement)* Compare rotation and previous position at apply time
- *(tablist)* Ignore replayed keep-alives and batch row updates per action mask
- *(scoreboard)* Order team leaves before joins and claim members and slots per viewer
- *(sidebar)* Remove the board with the component, stable widget handles
- *(sidebar)* Silence the objective removal on despawn and reset board state
- *(entity)* Seed attributes from the entity kind, effect routing nits
- *(entity)* Rebase attributes on every insert, reset dropped attributes, players count as living
- *(demo)* Bolt sway like the reference, width-only fade, port wording
- *(world-border)* Sanitize center, round ticks up, skip empty syncs
- *(world)* Explicit viewers take precedence over world time and weather
- *(commands)* Apply review nits on argument parsers and completion
- *(commands)* Never slice inside a char when computing completion range
- *(net)* Build zlib state lazily and cap compressed packets at 8 MiB
- *(commands)* Stop skipping flags after -- when walking subcommands
- *(vanilla-commands)* Check access before parsing, add online members by entity, resolve player UUIDs
- *(commands)* Read flags until a greedy argument begins and share one -- rule across walk, dispatch and completion
- *(commands)* Align completion greedy start with dispatch for nested commands
- *(commands)* Reject non-finite float and double arguments
- *(commands)* Restrict nearest-player selection to the executor dimension
- *(commands)* Validate the executor for player selectors
- *(commands)* Preserve flags after a missing value
- *(messages)* Test each text where it is built, not after the codec
- *(config)* A zero tick rate no longer panics at startup
- *(network)* Disconnect clients that stop answering keep-alives
- *(commands)* Completion agrees with dispatch on value flags followed by a flag
- *(spawn)* Spawn players on the surface instead of one block above it [**breaking**]
- *(spawn)* Keep surface_height_at above water in the bundled generators
- *(server)* Stop gracefully on SIGTERM and SIGHUP, not only Ctrl-C
- *(movement)* Relay players' crouch and on-ground flag to other players
- *(void)* Stream join chunks that the spawn area does not hold
- *(void)* Frame chunk sends in batches
- *(item-use)* Place the item held in the used hand when the click arrived
- *(world)* Stream chunks nearest-first and cap the join at the view distance
- *(physics)* Sweep the whole fall so fast entities cannot pass through thin floors
- *(biome)* Refuse late and malformed custom biome registrations
- *(entity)* Send a player's complete shared flags byte with its crouch
- *(interaction)* Decode 26.1.2 player commands and take sneaking from player input
- *(play)* Fire PlayerReadyEvent once per session
- *(network)* Pair PlayerQuitEvent with every PlayerJoinEvent
- *(players)* Close the connection on kick, after the reason is sent
- *(protocol)* Decode the chat acknowledgement and chat session update packets
- *(chat)* Refuse chat lines with illegal characters or over 256 characters
- *(commands)* Answer empty commands and a missing plugin list explicitly
- *(commands)* Refuse formatting codes in /tell and /broadcast messages
- *(movement)* Ignore client positions until a teleport is confirmed
- *(commands)* Refuse /give minecraft:air instead of claiming success
- *(item-drops)* Throw items along the look direction and collect them after vanilla's delay
- *(inventory)* Follow vanilla click rules and resync mispredicted slots
- *(item-use)* An empty hand places nothing
- *(item-use)* Run break hooks for the item held when the packet arrived
- *(config)* Replace negative view and simulation distances with zero
- *(metrics)* Name the TPS path and reason in every output warning
- *(players)* Tolerate a ready player despawned by another observer
- *(observability)* Count /kick and despawn closes as kicks after #151
- *(network)* Count a despawned client's close as a kick every time
- *(commands)* Gate admin defaults on Operator and send each player its permission level
- *(entities)* Settle on the whole collision box, remove entities fallen out of the world (#171, #172)
- Show players' equipment to others and apply vanilla equipment slot rules ([#170](https://github.com/VoidMinecraft/VoidMC/pull/170))
- *(protocol)* Decode the 26.1 Interact layout and the Attack packet
- *(entities)* Make EntityBuilder::wander(true) attach the wander walk ([#169](https://github.com/VoidMinecraft/VoidMC/pull/169))
- *(server)* Return the AppExit from VoidServer::run ([#179](https://github.com/VoidMinecraft/VoidMC/pull/179)) [**breaking**]
- *(registry)* Refuse registry changes once a client received them ([#183](https://github.com/VoidMinecraft/VoidMC/pull/183)) [**breaking**]
- *(movement)* Decode the status-only packet and ignore movement while a teleport is unconfirmed
- *(commands)* Reject /summon positions outside the spawnable bounds ([#173](https://github.com/VoidMinecraft/VoidMC/pull/173))
- Time out loading players and connections that never reach play
- *(item-use)* Place blocks like vanilla: replaceable target, entities, reach, items used, orientation
- *(world-io)* Quarantine a stored chunk that cannot be loaded ([#189](https://github.com/VoidMinecraft/VoidMC/pull/189))
- *(commands)* Refuse § codes in /say and kick reasons, strip them from echoed names
- *(chat)* Send player chat to every play-state client whose chat is shown
- *(network)* Disconnect clients that send a malformed known packet
- *(commands)* Greedy text keeps its spaces; log command arguments at debug only
- Send the default spawn position and always the join's own chunk
- *(server)* Count a kick whose client closed on its Disconnect first
- *(metrics)* Compare slow_tick_ms with the time a tick works
- *(observability)* Report stopping after the exit tick's gauges are stored
- *(chunk)* Send chunks nearest-first from the join on

### Chores

- *(void)* Add keepalive test
- *(block-entity)* Trim plugin module doc

### Documentation

- *(position)* Document TeleportEntity fallback, drop manual changelog entry
- *(server)* Document observers, send API and VoidSystems; strip comments
- *(entity)* Status effects and attributes reference
- Document Sneaking and the relayed on-ground flag of players
- *(observability)* Document the health states and metric set; enable them in the example with VOID_OBSERVABILITY_ADDR
- *(world)* Document how a custom generator drives spawn and terrain; deprecate generate_spawn_area
- Document TPS and flame output defaults, formats and failures
- *(commands)* Say when the tree is sent and how suggestion requests are answered
- Operator-only default commands, permission level and per-player tree

### Features

- Expand flame profiling tooling
- *(logging)* Log player and world events
- *(players)* Entity-addressed packet send API and migrate all call sites
- *(schedule)* Public VoidSystems system sets for framework phases
- *(boss-bar)* BossBar component, BossBarPlugin and Boss Event packet
- *(sounds)* Sound request API with Sound Effect, Entity Sound Effect and Stop Sound packets
- *(entity)* EntityBuilder, ghost-proof despawn and chunk-based visibility tracker
- *(biomes)* Cell-granular biome get/set, ChunksBiomes, generator hook and BiomeBuilder
- *(particles)* Fire-and-forget particle requests with chunk-viewer audience
- *(entity)* Entity metadata, display entities and passengers
- *(world)* Block entity storage, typed Sign/Skull/Banner, sync and persistence
- *(menu)* Server-side GUI menus, slot-granular inventory sync, held-slot and cooldown helpers
- *(entity)* Fluent EntityBuilder::with for pre-spawn components
- *(messages)* Messages/WorldMessages chat and action-bar API
- *(entity)* End Crystal metadata source
- *(player)* Server-authoritative position, chunk-send budget, flight setter and teleport barrier
- *(titles)* Titles API with title, subtitle, times and clear
- *(entity)* Hidden marker to hide and show tracked entities
- *(tablist)* Tab list header, footer and entry customisation
- *(scoreboard)* Sidebar objectives, scores and teams API
- *(sidebar)* Widget-based sidebar API
- *(entity)* Status effects and attribute modifiers
- *(demo)* Alpine Rush — minecart racing demo on the engine APIs
- *(world)* World border API
- *(world)* Time of day and weather API
- *(toasts)* Advancement toast notifications
- *(commands)* Typed Brigadier argument parsers and client-side completion
- *(net)* Negotiate compression at login and add compression_threshold config
- *(commands)* Subcommands (literal branches) in CommandBuilder
- *(items)* Before_break_block hook on dig start and ItemBehaviorRegistry::contains
- *(commands)* Requires() permission checks and keep dash tokens for flag-free and greedy arguments [**breaking**]
- *(messages)* A WireText holds only a text the client decodes [**breaking**]
- *(server)* Bind the listener before the game loop and expose ListenAddress
- *(void)* Opt-in multi-threaded ticks with region-parallel entity simulation
- *(observability)* Serve opt-in /health and Prometheus /metrics endpoints
- *(item-use)* Let blocks react to a right-click with BlockInteraction
- *(redstone)* Opt-in RedstonePlugin for levers, buttons, dust and note blocks
- Enforce max_players at login with vanilla's server-full reason
- *(example)* Make every player an operator by default

### Performance

- *(net)* Coalesce outbound writes into buffered per-batch flush
- *(entity)* Chunk-indexed visibility tracker
- *(net)* Bounded per-client queues with direct delivery
- *(net)* Single reusable outbound frame buffer

### Refactor

- *(players)* Hoist ready() snapshots, derive From impls, cover all sets
- *(boss-bar)* Audience on the component, clamp on the wire, allocation-free steady state
- *(sounds)* Dimension on Sound, at(dimension, pos), play reports real delivery
- *(biomes)* Re-export Tag, tolerant palette reads, register-before-login guard
- *(particles)* Explicit send(), borrowing patch encode, registry-resolved position sources
- *(messages)* Text replaces WireText, the sidebar widget becomes Span [**breaking**]
- *(item-use)* Read sneaking from the Sneaking component of #134

### Tests

- *(messages)* Build every oversized text through its production builder
- Make hollow unit tests assert something and drop duplicates
- VMC-ATP-024 check every ServerConfigBuilder option end to end
- *(config)* List execution_mode and observability after #138 and #145
- Cover status-only movement and movement sent before a teleport is confirmed
- Registries change after a client received them ([#183](https://github.com/VoidMinecraft/VoidMC/pull/183))
- Greedy command text keeps its spaces; command arguments stay out of the info log
- *(server)* A kick requested after the client closed on its Disconnect counts
- *(network)* Pin that a despawned client's kick is requested before its sender is dropped
- *(observability)* Read the telemetry between the writes of the exit tick

### Merge

- Feat/player-transfer into fix/passenger-broadcast-and-settle
- Fix/passenger-broadcast-and-settle into feat/entity-hidden
- Feat/entity-hidden into feat/effects-attributes
- Feat/effects-attributes into feat/world-border
- Feat/world-border into feat/time-weather
- Feat/command-arg-types into fix/messages-typed-color
- Feat/time-weather into feat/titles
- Feat/titles into feat/tab-list
- Feat/tab-list into feat/scoreboard
- Feat/scoreboard into feat/sidebar
- Feat/sidebar into feat/toasts
- Feat/toasts into feat/demo
- Fix/messages-typed-color into feat/toasts

### Style

- *(position)* Drop redundant comments
- *(position)* Trim comments

## [0.4.0](https://github.com/VoidMinecraft/VoidMC/compare/voidmc-protocol-v0.3.0...voidmc-protocol-v0.4.0) - 2026-11-21

### Bug Fixes

- *(commands)* Expose summon position and flags
- *(status)* Advertise correct protocol version
- *(interaction)* Synchronize multiplayer block changes
- *(network)* Bound packet framing and decoding
- *(entity)* Var-long metadata serializer, public MetadataSource registration, display clear()
- *(menu)* Full resync on title change, keep hidden inventory changes, creative-only clone, cursor return on disconnect, click packet hardening
- *(entity)* Passenger-aware player broadcast and settle snap delta
- *(entity)* Passenger-aware movement ingest, mount-guarded unmount and position sync resync
- *(data)* Entity attribute defaults, subclass mob effect particles, VarInt packet ids
- *(demo)* Bolt sway like the reference, width-only fade, port wording
- *(codec)* Checked arithmetic under a wider deny, a repr enum refuses a varint out of range, and limit tests
- *(codec)* A Decode cannot replace its decoder [**breaking**]
- *(movement)* Relay players' crouch and on-ground flag to other players
- *(void)* Frame chunk sends in batches
- *(protocol)* Send sections with more than 256 block states with global ids
- *(protocol)* Never truncate biome ids in direct palettes
- *(interaction)* Decode 26.1.2 player commands and take sneaking from player input
- *(protocol)* Decode the chat acknowledgement and chat session update packets
- *(protocol)* Switch an edited section to global ids past 256 states ([#168](https://github.com/VoidMinecraft/VoidMC/pull/168))
- *(commands)* Gate admin defaults on Operator and send each player its permission level
- Show players' equipment to others and apply vanilla equipment slot rules ([#170](https://github.com/VoidMinecraft/VoidMC/pull/170))
- *(protocol)* Decode the 26.1 Interact layout and the Attack packet
- *(server)* Return the AppExit from VoidServer::run ([#179](https://github.com/VoidMinecraft/VoidMC/pull/179)) [**breaking**]
- *(movement)* Decode the status-only packet and ignore movement while a teleport is unconfirmed
- Time out loading players and connections that never reach play
- *(chat)* Send player chat to every play-state client whose chat is shown
- Send the default spawn position and always the join's own chunk

### Chores

- *(boss-bar)* Adapt to bounded Decoder and register BossEvent in the packet-id guard

### Documentation

- *(server)* Document observers, send API and VoidSystems; strip comments
- *(observability)* Document the health states and metric set; enable them in the example with VOID_OBSERVABILITY_ADDR
- Document TPS and flame output defaults, formats and failures
- Operator-only default commands, permission level and per-player tree

### Features

- Expand flame profiling tooling
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
- *(player)* Server-authoritative position, chunk-send budget, flight setter and teleport barrier
- *(titles)* Titles API with title, subtitle, times and clear
- *(protocol)* Scoreboard objective, score and team packets
- *(tablist)* Tab list header, footer and entry customisation
- *(scoreboard)* Sidebar objectives, scores and teams API
- *(protocol)* Mob effect and attribute packets, Effect/EntityAttribute data enums
- *(demo)* Alpine Rush — minecart racing demo on the engine APIs
- *(world)* World border API
- *(world)* Time of day and weather API
- *(toasts)* Advancement toast notifications
- *(commands)* Typed Brigadier argument parsers and client-side completion
- *(protocol)* Clientbound login Set Compression packet
- *(net)* Zlib compressed framing in ClientReader/ClientWriter
- *(protocol)* Scoreboard command argument parsers (team, objective, score_holder, operation, scoreboard_slot, objective_criteria)
- *(protocol)* Clientbound waypoint packet for the locator bar
- *(protocol)* Clientbound section_blocks_update packet
- *(protocol)* Add the clientbound Block Event packet
- *(protocol)* Add the Take Item Entity clientbound packet (0x7C)
- Enforce max_players at login with vanilla's server-full reason
- *(example)* Make every player an operator by default

### Refactor

- *(players)* Hoist ready() snapshots, derive From impls, cover all sets
- *(boss-bar)* Audience on the component, clamp on the wire, allocation-free steady state
- *(biomes)* Re-export Tag, tolerant palette reads, register-before-login guard
- *(particles)* Explicit send(), borrowing patch encode, registry-resolved position sources

### Tests

- *(biomes)* Missing palette longs read as palette entry 0
- A section edited past 256 block states keeps every block ([#168](https://github.com/VoidMinecraft/VoidMC/pull/168))

### Merge

- Feat/player-transfer into fix/passenger-broadcast-and-settle
- Feat/entity-hidden into feat/effects-attributes
- Feat/effects-attributes into feat/world-border
- Feat/world-border into feat/time-weather
- Feat/time-weather into feat/titles
- Feat/titles into feat/tab-list
- Feat/tab-list into feat/scoreboard
- Feat/sidebar into feat/toasts
- Feat/toasts into feat/demo
- Fix/messages-typed-color into feat/toasts

## [0.2.0](https://github.com/VoidMinecraft/VoidMC/compare/voidmc-net-v0.1.1...voidmc-net-v0.2.0) - 2026-11-21

### Bug Fixes

- *(network)* Bound packet framing and decoding
- *(network)* Make connection I/O cancellation-safe
- *(demo)* Bolt sway like the reference, width-only fade, port wording
- *(net)* Build zlib state lazily and cap compressed packets at 8 MiB
- *(codec)* A Decode cannot replace its decoder [**breaking**]
- *(server)* Return the AppExit from VoidServer::run ([#179](https://github.com/VoidMinecraft/VoidMC/pull/179)) [**breaking**]

### Documentation

- *(observability)* Document the health states and metric set; enable them in the example with VOID_OBSERVABILITY_ADDR
- Document TPS and flame output defaults, formats and failures
- Operator-only default commands, permission level and per-player tree

### Features

- Expand flame profiling tooling
- *(demo)* Alpine Rush — minecart racing demo on the engine APIs
- *(net)* Zlib compressed framing in ClientReader/ClientWriter
- *(example)* Make every player an operator by default

### Performance

- *(net)* Coalesce outbound writes into buffered per-batch flush
- *(net)* Single reusable outbound frame buffer
- *(net)* Derive retained frame buffer from outbound limit, stack VarInt header

## [0.2.0](https://github.com/VoidMinecraft/VoidMC/compare/voidmc-data-v0.1.2...voidmc-data-v0.2.0) - 2026-11-21

### Bug Fixes

- *(block-entity)* Column-checked chunk storage, strict host asset, exact metadata strip on load
- *(data)* Allow too_many_arguments on emit_blocks_module
- *(data)* Entity attribute defaults, subclass mob effect particles, VarInt packet ids
- *(demo)* Bolt sway like the reference, width-only fade, port wording
- *(commands)* Apply review nits on argument parsers and completion
- Show players' equipment to others and apply vanilla equipment slot rules ([#170](https://github.com/VoidMinecraft/VoidMC/pull/170))
- *(server)* Return the AppExit from VoidServer::run ([#179](https://github.com/VoidMinecraft/VoidMC/pull/179)) [**breaking**]

### Chores

- *(data)* Restore full registries.json, ship packets.json and version.json

### Documentation

- *(observability)* Document the health states and metric set; enable them in the example with VOID_OBSERVABILITY_ADDR
- Document TPS and flame output defaults, formats and failures
- Operator-only default commands, permission level and per-player tree

### Features

- Expand flame profiling tooling
- *(data)* Generate packet ids and version info; guard protocol against silent drift
- *(entity)* EntityBuilder, ghost-proof despawn and chunk-based visibility tracker
- *(biomes)* Cell-granular biome get/set, ChunksBiomes, generator hook and BiomeBuilder
- *(data)* Block entity host table and type name lookup
- *(protocol)* Mob effect and attribute packets, Effect/EntityAttribute data enums
- *(entity)* Status effects and attribute modifiers
- *(demo)* Alpine Rush — minecart racing demo on the engine APIs
- *(commands)* Typed Brigadier argument parsers and client-side completion
- *(data)* Runtime block types with per-state property values
- *(data)* Vendor the #minecraft:replaceable block tag and expose block kinds ([#174](https://github.com/VoidMinecraft/VoidMC/pull/174))
- *(example)* Make every player an operator by default

### Refactor

- *(data)* Group emit_blocks_module inputs into a struct

### Merge

- Feat/toasts into feat/demo

## [0.2.0](https://github.com/VoidMinecraft/VoidMC/compare/voidmc-codec-v0.1.1...voidmc-codec-v0.2.0) - 2026-11-21

### Bug Fixes

- *(network)* Bound packet framing and decoding
- *(data)* Entity attribute defaults, subclass mob effect particles, VarInt packet ids
- *(demo)* Bolt sway like the reference, width-only fade, port wording
- *(codec)* Typed decode errors instead of panics, and a crate-wide deny on panicking code
- *(codec)* Checked arithmetic under a wider deny, a repr enum refuses a varint out of range, and limit tests
- *(codec)* Max_nbt_depth counts one level per compound or list
- *(codec)* Never send an NBT string the client cannot read
- *(codec)* One width rule, a named root, and a bench for the NBT encoder
- *(codec)* Cap max_nbt_depth at 512 levels with an NbtDepth type [**breaking**]
- *(codec)* A Decode cannot replace its decoder [**breaking**]
- *(server)* Return the AppExit from VoidServer::run ([#179](https://github.com/VoidMinecraft/VoidMC/pull/179)) [**breaking**]
- *(network)* Disconnect clients that send a malformed known packet

### Documentation

- *(observability)* Document the health states and metric set; enable them in the example with VOID_OBSERVABILITY_ADDR
- Document TPS and flame output defaults, formats and failures
- Operator-only default commands, permission level and per-player tree

### Features

- Expand flame profiling tooling
- *(protocol)* Mob effect and attribute packets, Effect/EntityAttribute data enums
- *(demo)* Alpine Rush — minecart racing demo on the engine APIs
- *(example)* Make every player an operator by default

### Performance

- *(codec)* Write NBT straight into the packet buffer, one cut rule
- *(codec)* Win back the decode cost of the wider deny

### Tests

- *(codec)* A decode bench for varints, a movement packet and a chat NBT
- *(codec)* Pin the sealed decoder, and say what a scope is not for
- Make hollow unit tests assert something and drop duplicates

### Merge

- Feat/toasts into feat/demo

## [0.2.0](https://github.com/VoidMinecraft/VoidMC/compare/voidmc-codec-macros-v0.1.1...voidmc-codec-macros-v0.2.0) - 2026-11-21

### Bug Fixes

- *(network)* Bound packet framing and decoding
- *(data)* Entity attribute defaults, subclass mob effect particles, VarInt packet ids
- *(demo)* Bolt sway like the reference, width-only fade, port wording
- *(codec)* Checked arithmetic under a wider deny, a repr enum refuses a varint out of range, and limit tests
- *(server)* Return the AppExit from VoidServer::run ([#179](https://github.com/VoidMinecraft/VoidMC/pull/179)) [**breaking**]
- *(network)* Disconnect clients that send a malformed known packet

### Documentation

- *(observability)* Document the health states and metric set; enable them in the example with VOID_OBSERVABILITY_ADDR
- Document TPS and flame output defaults, formats and failures
- Operator-only default commands, permission level and per-player tree

### Features

- Expand flame profiling tooling
- *(protocol)* Mob effect and attribute packets, Effect/EntityAttribute data enums
- *(demo)* Alpine Rush — minecart racing demo on the engine APIs
- *(example)* Make every player an operator by default

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
