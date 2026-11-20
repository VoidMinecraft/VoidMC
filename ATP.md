# VoidMC ATP: objectives tracker

Extracted from `atp_679_VoidMC.xlsx` (sheet "Test Plan"). Local working file: **do not commit**.

Target: Minecraft Java Edition 26.1.2 / protocol 775. Local server: `127.0.0.1:25565` via `cargo run -p voidmc-example`.

`ATP status` is the status written in the spreadsheet and may be stale. `Verified status` is filled in after checking the code and tests:

- `Not checked`: not yet looked at
- `Implemented`: behavior matches the expected results and is covered by tests
- `Partial`: some expected results are missing or wrong (fix in the PR)
- `Missing`: not implemented (implementation in the PR)
- `Blocked`: can't be verified or implemented without a human decision (reason in Notes)

## Summary

| ID | Feature | Category | Depends on | ATP status | Verified status | PR | Merged | Notes |
|---|---|---|---|---|---|---|---|---|
| VMC-ATP-001 | Start the example server | Critical feature | - | Done | Implemented | [#129](https://github.com/VoidMinecraft/VoidMC/pull/129) | Yes (2 of 37, 12:41 UTC) | Tests only. New `void-example/tests/startup.rs` launches the built example binary and checks the listening line, 127.0.0.1:25565, "Generated 81 spawn area chunks", a live login reply and no ERROR/panic. Crate-level test because the in-process e2e harness cannot reach the example binary. Runs by default and needs port 25565 free (fails with a clear message otherwise). "Remains running" is checked once after startup. |
| VMC-ATP-002 | Display the server status | Critical feature | 001 | Done | Partial | [#130](https://github.com/VoidMinecraft/VoidMC/pull/130) | Yes (10 of 37, 13:47 UTC) | Fix + tests. Framework status path is correct (26.1.2, protocol 775, ping/pong, live player count), but the example server set its own MOTD "Void Example Server", so the ATP's "Welcome to Void Server!" was never shown. Judgment call: ATP taken as the spec, example MOTD changed to the framework default; example config extracted into `server_config()` so a unit test can pin it. Two e2e scenarios added. Revert the MOTD line if the example name was intentional. |
| VMC-ATP-003 | Complete the implemented login and configuration handshake | Critical feature | 001 | Done | Implemented | [#131](https://github.com/VoidMinecraft/VoidMC/pull/131) | Yes (11 of 37, 13:56 UTC) | Tests only. One e2e scenario drives an Azalea connection by hand and is the retained packet trace the ATP feedback asked for: exact client and server traces for Handshake/Login/Configuration, ordered milestones for Play, ready only after PlayerLoaded. Assumption: "no compression claimed or required" tested with compression off (default config enables it at threshold 256; compressed joins are covered by existing tests). Out of scope, not fixed: ClientInformation is handled by two identical observers. |
| VMC-ATP-004 | Load the initial world | Critical feature | 003 | Done | Partial | [#136](https://github.com/VoidMinecraft/VoidMC/pull/136) | Yes (3 of 37, 12:48 UTC) | Fix + tests. **Based on #135 (merge #135 first):** it needs that PR's fix for the holes left around spawn when the spawn area is smaller than the join radius. Own fix: chunks are now framed in ChunkBatchStart/Finished (join and streaming) and the client's ChunkBatchReceived is accepted instead of logged as unrecognized. Four e2e scenarios (initial area, 360° turn, 40 blocks out and back, world intact on return). Assumptions: batch framing treated as required because vanilla always sends it; the client-reported batch rate is ignored. Out of scope, not fixed: SetDefaultSpawnPosition never sent; status-only movement packet not decoded; a negative `spawn_chunk_radius` would leave a client on the loading screen; e2e bot does not answer ChunkBatchFinished by itself. |
| VMC-ATP-005 | Synchronize two players | Critical feature | 003 | Done | Partial | [#134](https://github.com/VoidMinecraft/VoidMC/pull/134) | Yes (4 of 37, 12:57 UTC) | Fix + tests, CI green. Follow-up pushed after review: the crouch packet carried a shared-flags byte with only the sneaking bit. The suspected clash with glowing/invisible updates did not exist (players' glowing/invisible bits were never sent to other players at all), so the follow-up makes one function own the byte and — NEW BEHAVIOUR, coordinator-prompted, beyond the ATP text — a player's glowing and invisibility flags now reach other players, complete with the sneaking bit. A player is still not sent its own flags. #147 (stacked on this) still merges cleanly. Spawn, tab list, walking, yaw/head yaw and leaving worked. Broken: crouching was never sent to other players (no SetEntityData), and every relayed movement hardcoded `on_ground: true`, so jumps never left the ground. Fix adds a `Sneaking` component and a crouch/pose broadcast (also for late joiners) and relays the client on-ground flag. Three e2e scenarios, both directions. Assumptions: crouch read from Player Input (how 26.1.2 clients report it); pose is crouching unless flying. Out of scope, NOT fixed: (1) `PlayerCommandAction` in void-protocol uses pre-1.21.2 numbering, so a real client's sprint is decoded as StopSneaking; (2) the status-only movement packet (`SetPlayerMovementFlags`) is not decoded. |
| VMC-ATP-006 | Modify shared blocks | Critical feature | 005 | Done | Partial | [#137](https://github.com/VoidMinecraft/VoidMC/pull/137) | Yes (6 of 37, 13:14 UTC) | Fix + tests, CI green. Second follow-up pushed: with the off-hand change, the empty off-hand packet of a vanilla click ran default placement for "air" and replaced the neighbouring block with air (found by the 022 subagent); an empty hand now places nothing, with a unit test and an e2e scenario. Follow-up pushed: a `void-worldedit` test relied on the old off-hand bug and was updated (CI green). User-visible side effect: with a WorldEdit wand/brush or the navigation remote in the main hand and a block in the off hand, a right-click now also places the off-hand block (vanilla behaviour for that packet); suppressing it would need a new server rule. Breaking, broadcast, convergence, out-of-view changes and invalid interactions were correct. Placement bugs fixed: (1) the placed item was read when the use queue drained, so a hotbar change in the same tick placed the wrong block; (2) an off-hand use placed the main-hand item, so a block held only in the off hand was never placed. Six e2e scenarios + one unit test. `enqueue_use_on_block`/`enqueue_use_in_air` gain an inventory parameter (crate-internal use only). Assumption: Creative = server `game_mode` config; `/gamemode` only changes the displayed mode. Left as is (consistent for all players, but not vanilla): placing into an occupied block overwrites it; no reach check; default placement never decrements the stack in Survival; a malformed play packet is logged at ERROR instead of disconnecting. May conflict with 022 (item behaviors). |
| VMC-ATP-007 | Exchange chat messages | Secondary function | 005 | Done | Partial | [#152](https://github.com/VoidMinecraft/VoidMC/pull/152) | Yes (31 of 37, 17:55 UTC) | Fix + tests, CI green. Plain chat already reached every ready player once, in the chat box, as `<Name> text`, in one consistent order. Fixed: (1) a player could restyle or forge lines (`§` codes were applied by clients; a line break could fake another player's line; no length limit) — lines with `§`, control characters or more than 256 UTF-16 units are now refused with a red message to the sender only; (2) the chat session update and chat acknowledgement packets a real client sends were logged as unrecognised — now decoded (two new serverbound packets). Six e2e scenarios incl. signed chat packets and an 80-line burst. Judgment call: illegal chat is refused rather than kicked (vanilla kicks) because the ATP says chat must not disconnect anyone. Out of scope, not fixed: chat goes out as system chat so a client's "commands only"/"hidden" chat setting is not honoured; a player still loading misses chat sent in that window. |
| VMC-ATP-008 | Discover available commands | Secondary function | 003 | Done | Partial | [#153](https://github.com/VoidMinecraft/VoidMC/pull/153) | Yes (23 of 37, 15:54 UTC) | Fix + tests, CI green. `/help`, `/ping`, `/plugins` and unknown commands were answered correctly, to the sender only, through both command paths. Fixed: (1) `/` alone (an empty command, as the vanilla client sends it) got no reply; now `Unknown command: /`; (2) `/plugins` with no PluginList (every server unless the app inserts one, incl. the example) said "No plugin list available."; now the explicit empty answer "No plugins loaded.". Eight e2e scenarios with exact reply text, both packet paths, invalid input incl. control characters and 32 000-character names, a not-yet-ready player, two players in one tick. commands.md corrected (descriptions `/help` really prints, PluginList is app-supplied names, new invalid-input table). Documented rather than changed: names are case-sensitive, `/help` does not list aliases and has no pages. Out of scope, not fixed: `§` codes in echoed unknown-command names are rendered as formatting (sender only); every command is logged with full arguments at info level. |
| VMC-ATP-009 | Preserve a world change after restart | Critical feature | 003, 006 | Done | Implemented | [#140](https://github.com/VoidMinecraft/VoidMC/pull/140) | Yes (14 of 37, 14:19 UTC) | Tests only. Three e2e scenarios: a player places blocks (including an oak log in a non-default state) in four chunks across four region files, clean stop, two restarts, every block and its surroundings identical and every region slot readable; a change reverted after a save stays reverted; a later session writes into an existing region file without damaging it. Assumptions: built-in placement only places default states, so the oriented log uses a test ItemBehavior through the real use-item path; clean stop = the harness's AppExit (`/stop` and signals are covered by #132). Same e2e dev-dependency line as #127/#132 (trivial conflict). Out of scope, unverified: possible lost dirty mark for block-entity sync when marked and saved in the same tick; an unreadable chunk is logged and silently regenerated. |
| VMC-ATP-010 | Change a player's displayed game mode | Secondary function | 003 | Done | Implemented | [#155](https://github.com/VoidMinecraft/VoidMC/pull/155) | Yes (24 of 37, 16:06 UTC) | Tests + docs only, CI green (the command description is left exactly as on main, to stay compatible with #153's pinned `/help` output). Three e2e scenarios: each of modes 0–3 (and names, and `/gm`) gives exactly one game-mode event and confirmation to the sender and one tab-list update to every player; invalid values get an exact parse error and change nothing; on a Survival server the server keeps applying Survival rules after `/gamemode 1` and a rejoin resets the displayed mode. Docs now state the command changes the displayed mode only. Notes: no Player Abilities packet is sent (the client derives abilities from the event); numeric ids show red in a vanilla client while typing but are accepted. Out of scope, NOT fixed: `SetCreativeModeSlot` is accepted from any player whatever the configured mode, so a Survival player can write any item into its own inventory. |
| VMC-ATP-011 | Teleport a player | Secondary function | 003 | Done | Partial | [#156](https://github.com/VoidMinecraft/VoidMC/pull/156) | Yes (32 of 37, 18:10 UTC) | Fix + tests, CI green. `/tp` already sent exact absolute coordinates with a fresh teleport id, kept yaw/pitch, streamed the destination chunks and moved the same entity for other players. Fixed: (1) a teleport could be undone before the client accepted it — stale position packets from the old spot overwrote the server position and other players saw the jump back; positions are now ignored while a teleport is unconfirmed (rotation still applies); (2) `/tp 1e300 64 0` panicked the game thread (integer overflow in chunk streaming), disconnecting everyone; `/tp` now rejects destinations beyond vanilla's limits (±30,000,000 horizontal, ±20,000,000 vertical). Four e2e scenarios; harness gains `Bot::hold_teleports`. Notes: an unanswered teleport is not resent (vanilla resends every 20 ticks), so a client that never accepts stays put with its positions ignored; every player is visible to every other regardless of distance on main, so no remove/add on long teleports. Conflicts expected with #134 in movement.rs. |
| VMC-ATP-012 | Manage connected players | Secondary function | 005 | Done | Partial | [#154](https://github.com/VoidMinecraft/VoidMC/pull/154) | Yes (30 of 37, 17:41 UTC) | Fix + tests, CI green. **Based on #151 (merge #151 first):** on main `/kick` only sent the Disconnect packet and never closed the connection; #151 adds the server-side close. With it, `/list`, `/tell` (`/msg`), `/broadcast` and `/kick` behave as documented and replies reach only the right players. Fixed here: `/tell` and `/broadcast` passed `§` formatting codes through, so a player could restyle a broadcast or whisper; text with `§` or control characters is now refused (same rule and wording as #152 for chat). Four e2e scenarios incl. a kicked client that ignores Disconnect, reconnect, a not-yet-ready player, and `/list` vs tab list vs status count. Notes: "online" = ready players (documented); a kicked player can still appear in `/list` for about one tick. Out of scope, not fixed: NO PERMISSION CHECK on the default commands — any player can `/kick` anyone, also on the example server (design decision); runs of spaces in `/tell`/`/broadcast` text collapse to one; `/say` and kick reasons still pass `§` codes. |
| VMC-ATP-013 | Give and clear inventory items | Secondary function | 003 | Done | Partial | [#157](https://github.com/VoidMinecraft/VoidMC/pull/157) | Yes (25 of 37, 16:18 UTC) | Fix + tests, CI green. `/give` and `/clear` met every expected result: counts land in valid stacks, stack limits hold (20 ender pearls → 16 + 4), `/clear` empties all 46 slots and the cursor, and the client view equals the server slot by slot. One bug fixed: `/give minecraft:air` gave nothing but replied "Gave 1 x minecraft:air"; now refused. Six e2e scenarios (ATP steps as written, stack limits and fill order, capacity, 16 bad inputs, full clear, rejoin). Documented rather than changed: count is limited to 1..64 per command (vanilla allows more); items that do not fit are NOT given and the reply says how many (vanilla drops them); `/give` has no target argument; the inventory resets on reconnect (no player-data persistence). Out of scope, not fixed: no SetEquipment packet is ever sent, so other players never see a held item or armor; no permission check on default commands. |
| VMC-ATP-014 | Manipulate inventory slots | User interface | 003 | Done | Partial | [#159](https://github.com/VoidMinecraft/VoidMC/pull/159) | Yes (37 of 37, 21:46 UTC; first skipped, then merged after the owner's decision) | Fix + tests, CI green. All click modes existed but several did not follow vanilla, and the server ignored the client's hashed prediction, so a slot the client got wrong stayed wrong. Fixed: mispredicted slots/cursor are resent and correct predictions are no longer echoed (state id no longer advances on every click); double-click takes from partial stacks first; drags follow vanilla's packet order and never duplicate; unknown buttons/modes and out-of-range slots do nothing (an out-of-range slot used to drop the whole cursor stack); crafting output slot is take-only; closing the inventory returns the cursor stack and crafting grid (they used to linger invisibly); `max_stack_size` component respected; and `SetCreativeModeSlot` is only honoured on a Creative server for slots 1–45 (it let a Survival player create any stack — the bug 010 reported). Twelve e2e scenarios checked against an independent model of vanilla's click rules, incl. 400 seeded random click batches with item conservation. NOT supported, documented: no equipment data, so no armor/shield routing on shift-click and armor slots accept any item. Conflicts expected in the creative-slot handler with #137 and #158 (which makes slot -1 drop the stack): keep both rules when merging. |
| VMC-ATP-015 | Drop and collect an item | Secondary function | 005 | Done | Partial | [#158](https://github.com/VoidMinecraft/VoidMC/pull/158) | Yes (26 of 37, 16:32 UTC) | Fix + tests, CI green. Dropped items already became entities with item data, fell and could be picked up. Fixed: (1) a player who dropped an item while standing still took it back half a second later (item spawned at the feet with no throw, 10-tick delay) — items are now thrown from eye height along the look direction, slow down and rest about 2.3 blocks ahead, with vanilla's 40-tick pickup delay; (2) no pickup animation — new `TakeItemEntity` packet (0x7C); (3) a player with a full inventory blocked the next player in range; (4) items thrown out of the creative inventory disappeared. Five e2e scenarios with a per-tick recorder that checks item conservation. Judgment calls: delay 40 ticks for every drop; pickup reach kept as a 1.5-block sphere (vanilla uses a box); no random throw spread; items keep the shared entity gravity (vanilla items fall slower); the animation reports the amount actually taken. Out of scope, not fixed here: a 0.25-high item can fall through a one-block floor after a ~10-block drop on main (#144 fixes it for all entities). Touches the creative-slot handler in item_use.rs (check against #137 and 014). |
| VMC-ATP-016 | Spawn a supported entity | Secondary function | 003 | Done | Implemented | [#141](https://github.com/VoidMinecraft/VoidMC/pull/141) | Yes (15 of 37, 14:27 UTC) | Tests only. Two e2e scenarios: a summoned pig is decoded as a pig by Azalea, at the right position, with fresh distinct id/UUID, stable for 40 ticks, seen once by a second player, a late joiner and a player leaving and re-entering range; 17 invalid forms (malformed id, unknown type, player/fishing_bobber, bad coordinates, unknown flag) each give a red error and spawn nothing. Assumption: bare `pig` is rejected as the docs say (vanilla accepts it). Out of scope, not fixed: far out-of-bounds coordinates (e.g. x=1e9) are accepted where vanilla rejects them. |
| VMC-ATP-017 | Apply wandering behavior to an entity | Secondary function | 016 | Done | Implemented | [#143](https://github.com/VoidMinecraft/VoidMC/pull/143) | Yes (17 of 37, 14:46 UTC) | Tests only, CI green. Two e2e scenarios record the server every tick and replay every packet a client received: 0.08 blocks/tick, yaw faces the step, heading redrawn every 40..139 ticks, client position never drifts from the server; a flagless pig, side by side, gets none of it; 33 wanderers incl. one in unloaded chunks and a watcher leaving and returning cause no error. Assumption: "documented Wander behavior" = the values `/summon` uses (docs only say "demo random-walk"). Out of scope, NOT fixed: the public `EntityBuilder::wander(true)` only sets a flag no system reads, so an entity built that way never wanders (`/summon` works because it inserts the `Wander` component itself); wanderers resend rotation every tick. |
| VMC-ATP-018 | Apply gravity and block collisions | Critical feature | 016 | Done | Partial | [#144](https://github.com/VoidMinecraft/VoidMC/pull/144) | Yes (18 of 37, 14:59 UTC) | Fix + tests, CI green. Landing, grounded state, stable rest, one-block step-up, wall collisions and opt-in flags worked. Bug fixed: an entity falling faster than ~1.9 blocks/tick (a pig after a ~24-block drop) passed straight through a one-block-thick floor, because gravity only checked the box at the end of the tick's fall. Four e2e scenarios (pen in the air built by the test, pigs summoned with the real command, whole collision box checked every tick, second client decodes on-ground flags) + exhaustive unit tests. What the tests steer: in the deterministic scenarios the pig's Wander yaw is set and its timer frozen; the random scenario steers nothing. Will conflict mechanically with #138 in physics.rs. Out of scope, not fixed: spawn settling only looks at the centre column and can snap one block into the ground (masked by step-up); an entity with `--gravity` but no `--block-checks` falls forever and is never removed. |
| VMC-ATP-019 | Stream chunks around a moving player | Critical feature | 003 | Done | Partial | [#139](https://github.com/VoidMinecraft/VoidMC/pull/139) | Yes (28 of 37, 17:00 UTC) | Fix + tests. Capping as min(client, server), forgetting behind the player and centre-before-chunks already worked. Fixed: (1) the join sent the whole `initial_chunk_radius` square even when the effective view distance was smaller, then forgot the extra ring; (2) nearest-first broke when the generation cap held back a nearer ungenerated chunk while farther cached chunks were sent. Seven e2e scenarios + one unit test. Judgment calls: nearest-first = squared Euclidean distance from the player's chunk (as documented); cached chunks now wait behind nearer ungenerated ones, which lowers throughput under the generation cap (vanilla sends whatever is ready). Based on main; will conflict trivially with #135/#136 on one line of the join loop. Combined behavior of #135 + #136 + #139 has not been tested together. |
| VMC-ATP-020 | Load chunks through the world loader hook | Critical feature | - | Done | Implemented | [#127](https://github.com/VoidMinecraft/VoidMC/pull/127) | Yes (8 of 37, 13:32 UTC) | Tests only. Two e2e scenarios: test ChunkLoader consulted before generator (spawn area and streaming), and WorldPersistencePlugin/RegionChunkStore round-trip across a restart. Assumption: "documented generator fallback" = active generator output; only block states compared. Adds voidmc-world-io as an e2e dev-dependency (trivial conflict risk in void-e2e/Cargo.toml and Cargo.lock). |
| VMC-ATP-021 | Register a custom command | Secondary function | - | Done | Implemented | [#133](https://github.com/VoidMinecraft/VoidMC/pull/133) | Yes (13 of 37, 14:11 UTC) | Tests only, plus a one-line docs fix (missing `StringArg` import in the CommandBuilder example). Two e2e scenarios: a command registered with `add_command` is in the tree and `/help` and runs, then the same app restarted without it gives "Unknown command"; same checks for a command registered from a plugin. Out of scope, not fixed: commands.md says the tree is sent "during the configuration phase", it is sent on entering Play. |
| VMC-ATP-022 | Register a custom item behavior | Secondary function | - | Done | Partial | [#160](https://github.com/VoidMinecraft/VoidMC/pull/160) | Yes (7 of 37, 13:23 UTC) | Fix + tests, CI green. **Based on #137 (merge #137 first):** without #137's per-hand snapshot one vanilla click runs the stick behavior twice. The ItemBehavior API is public and documented and worked for a single packet, but with a real vanilla click (three packets: use-on main hand, use-in-air, use-on off hand) the example wand did nothing useful: the empty off hand ran the default placement for the air item and placed air over the wand's glowstone (that guard now lives in #137). Bug fixed here: the break hooks used the item held when the queue drained, so a same-tick hotbar change ran another item's behavior. Eight e2e scenarios (full vanilla click, context contents, Pass vs Handled, consume, break hooks, unrelated items unchanged, replace-on-second-registration, unknown item → warning, bursts) + two unit tests. `enqueue_break`/`enqueue_start_break` gain an inventory parameter. Notes: one vanilla click on a block runs both `on_use_on_block` and `on_use` (as vanilla; documented, not suppressed); a panicking behavior takes down the game thread (untested, undocumented). Out of scope, not fixed: `place_block` and the default placement overwrite a solid neighbour (vanilla only replaces replaceable blocks). |
| VMC-ATP-023 | Extend the server through the Bevy plugin hook | Secondary function | - | In progress | Implemented | [#128](https://github.com/VoidMinecraft/VoidMC/pull/128) | Yes (9 of 37, 13:39 UTC) | Tests only; ATP status "In progress" was stale. One e2e scenario: plugin closure inserts resources, a Startup and an Update system, an observer and a /probe command; client sees the command in the join tree and the observer replies. "No Java/Spigot loader implied" checked by reading the docs, not by a test. |
| VMC-ATP-024 | Configure the server through ServerConfigBuilder | Secondary function | - | Done | Partial | [#161](https://github.com/VoidMinecraft/VoidMC/pull/161) | Yes (36 of 37, 20:05 UTC) | Fix + tests, CI green. Every builder option already reached something observable and documented defaults matched the code. Fixed: a negative `view_distance` was sent to the client unchanged and left the player standing in the void (negative view/simulation distances are now replaced by 0 with a warning). Docs corrected: `max_players` is advertised only, NOT enforced (a test shows two players online with `max_players(1)`); clients are not told the tick rate; new "Limits" section stating configuration guarantees neither capacity nor performance. Seven e2e scenarios (every option set to a non-default value and observed by a client; tick rate 200 vs 10 TPS; defaults restored in the same process) + an integration test that parses the configuration reference table and compares it with the code. Judgment calls: `max_players` documented as advisory rather than adding a login refusal; an unbindable/unparsable address still panics before the first tick (documented). AFTER MERGE of #138 or #145: `void/tests/server_config.rs` will not compile until the new config field is added to its list (intended guard); rerun CI. |
| VMC-ATP-025 | Capture TPS and profiling diagnostics | Secondary function | - | Done | Partial | [#162](https://github.com/VoidMinecraft/VoidMC/pull/162) | Yes (35 of 37, 19:49 UTC) | Fix + tests, CI green. The TPS CSV and the flame trace were written correctly. Fixed: (1) an unusable flame output path aborted startup with an error instead of a warning; now a warning naming the path and reason, and the server runs on; (2) `VOID_METRICS_MODE=flame` alone also wrote an undocumented TPS CSV; TPS sampling now depends on `VOID_METRICS_DEBUG` only; (3) the TPS write-failure warning named neither path nor reason; (4) docs out of step (default paths, sampling interval, replace-not-append). Tests launch the real example binary as a child process on an ephemeral port through a new `VOID_LISTEN_ADDR` variable (default unchanged, 127.0.0.1:25565), stop it with SIGINT and check the CSV and folded trace are complete; plus unit tests and three in-process e2e scenarios. Notes: an existing CSV is replaced; the window in progress at shutdown is not written. Out of scope: on main SIGTERM loses the last trace buffer (#132 fixes graceful SIGTERM). Conflicts expected in `void-example/src/main.rs` with #130, #145, #147. |
| VMC-ATP-026 | Stop the server gracefully | Critical feature | 001 | Done | Partial | [#132](https://github.com/VoidMinecraft/VoidMC/pull/132) | Yes (12 of 37, 14:04 UTC) | Fix + tests. `/stop` and SIGINT worked; SIGTERM, documented as graceful, killed the process without flushing dirty chunks (the `ctrlc` crate only handles SIGINT without its `termination` feature). Fix enables that feature in `void/Cargo.toml`; SIGHUP becomes graceful too and is documented. Tests: process-level SIGINT/SIGTERM/SIGHUP tests in `void-world-io/tests/graceful_stop.rs` (own server process, ephemeral port, not the example binary) and an e2e `/stop` scenario. Harness gains `TestServer::wait_for_exit()`; same e2e dev-dependency line as #127 (trivial conflict). Not changed: `VoidServer::run` drops the AppExit value, so the process always exits 0. |
| VMC-ATP-027 | Generate optimized worlds | Critical feature | 026 | In progress | Partial | [#135](https://github.com/VoidMinecraft/VoidMC/pull/135) | Yes (1 of 37, 12:11 UTC) | Fix + tests. Assumption: no separate "optimized" generator or switch exists, so "optimized path" = the default pipeline (spawn-area pre-generation, chunk cache, per-tick generation budget); reference path = no pre-generation and no budget. No speed-up measured or claimed. Bug fixed: when the spawn area is smaller than `initial_chunk_radius` and the view distance equals it, joining left holes around spawn until the player moved. Terrain fixture defined in the e2e scenario (column rule, heights, spawn (0,64,0), seams); optimized and reference paths proven identical. Out of scope, not fixed: public `generate_spawn_area`/`generate_chunk`/`surface_height_at` free functions ignore the configured generator. |
| VMC-ATP-028 | Process regions and systems in parallel | Critical feature | 027 | In progress | Missing | [#138](https://github.com/VoidMinecraft/VoidMC/pull/138) | Yes (33 of 37, 19:06 UTC) | Implementation + tests. On main nothing ran in parallel: `bevy_ecs` was built without `multi_threaded`, so all schedules were single-threaded (the e2e workspace got the multi-threaded executor by accident through Azalea's feature unification). Added `ExecutionMode { SingleThreaded (default), MultiThreaded }` via `ServerConfigBuilder::execution_mode`, and `for_each_region` (region = 32x32 chunks of one dimension) used by entity physics and spawn settling. Tests prove concurrency with a rendezvous (not timing), byte-for-byte equality with the single-thread baseline, and exactly-once updates. Judgment calls: default stays single-threaded; only existing per-entity work is region-parallel; wander is not (uses thread RNG). KNOWN LIMIT, not fixed: Bevy reports 26 unordered conflicting system pairs in `Update`, so under MultiThreaded their order within a tick can differ from the baseline; documented in configuration.md, needs a decision on a total order. |
| VMC-ATP-029 | Simulate biomes and weather | Secondary function | 027 | In progress | Implemented | [#146](https://github.com/VoidMinecraft/VoidMC/pull/146) | Yes (19 of 37, 15:09 UTC) | Tests + docs only, CI green; ATP status "In progress" was stale. Two e2e scenarios on a fixture (a snowy_plains area in a plains world, clear → thunder over a 100-tick ramp, with a mid-ramp reversal): registry carries the biomes' client-visible properties, every biome cell the client holds matches, a traveller sees the area biome and area-scoped weather on entering and clear on leaving, and each client's weather equals the server's at every tick incl. a mid-ramp joiner. Assumptions: "configured biome" = a built-in biome placed by the generator; "configured transition" = the level ramp plus a change mid-ramp; area weather = an explicit audience kept by an application system (weather has no position). Docs gain explicit "not simulated" sections. Small shared harness change: `JoinTranscript.registry_data`. |
| VMC-ATP-030 | Trigger note blocks with basic redstone | Secondary function | 027 | In progress | Missing | [#147](https://github.com/VoidMinecraft/VoidMC/pull/147) | Yes (5 of 37, 13:05 UTC) | Implementation + tests, CI green. **Based on #134 (merge #134 first):** it needs that PR's `Sneaking(bool)` component for the sneak rule on lever/button clicks. Nothing existed on main. Added an opt-in `RedstonePlugin` (not in DefaultPlugins because it changes what a right-click on a lever/button does; the example enables it and adds `/circuit` to build the test circuit): levers and all buttons (20/30-tick release), flat redstone dust with power 15 decreasing by 1, note block plays once per rising edge (Sound + new Block Event packet 0x07), silent when covered, states stored so rejoiners see them. New public `BlockInteraction` trait/registry and `world::set_block_state`. Six e2e scenarios + 16 unit tests. Not supported (documented): dust up/down a block, power through solid blocks, torches, repeaters, comparators, pistons, tuning by right-click. Deviation from vanilla: registered item behaviors run before the block interaction, to keep the WorldEdit wand and navigation remote working. Out of scope, not fixed: hand placement ignores orientation/shape (a hand-placed lever is always a north wall lever, dust a dot). |
| VMC-ATP-031 | Run functional and load tests | Critical feature | 027 | In progress | Missing | [#142](https://github.com/VoidMinecraft/VoidMC/pull/142) | Yes (16 of 37, 14:36 UTC) | Implementation + tests. On main only a TCP-connect stress POC existed (no login). Added a load runner in `void-e2e` (`voidmc-load` binary) reusing the full-protocol e2e bot: 7 smoke checks, then a load phase; versioned workloads `smoke@1` (4 clients, CI-sized) and `standard@1` (20 clients, 30 s, opt-in) with thresholds; in-process target by default or an operator-supplied endpoint (refuses to run without one, no default address); JSON + Markdown report with workload, run conditions, measurements, per-threshold verdicts, decision and a scope statement; exit code 0/1/2. Eight e2e scenarios incl. real induced failures. Judgment calls: latency = chat/command round trips as seen by clients; tick rate from world-time updates (the in-process run spawns a world clock; not applicable for an endpoint that sends none); thresholds chosen by the subagent, not from the ATP. Removed an unbacked "thousands of concurrent connections" claim from getting-started.md. Out of scope, not fixed: nothing refuses a login when the server is full (`max_players` is only advertised). |
| VMC-ATP-032 | Expose health and Prometheus metrics | Secondary function | 027 | In progress | Missing | [#145](https://github.com/VoidMinecraft/VoidMC/pull/145) | Yes (34 of 37, 19:30 UTC) | Implementation + tests, CI green. Nothing existed on main. Added to the core crate, off by default: `ServerConfigBuilder::observability(ObservabilityConfig::localhost(port))`; example reads `VOID_OBSERVABILITY_ADDR`. `GET /health`: `starting`/`stalled`/`stopping` → 503, `ok` → 200; after stop the port refuses connections. `GET /metrics`: Prometheus text 0.0.4 with tick (count, duration histogram, target and measured TPS), player (online, max, joined, connections) and error (connection errors by kind, kicks, unrecognized packets, packet errors) metrics. HTTP runs on its own thread, no new dependencies. Seven e2e scenarios with a strict parser. Judgment calls: the "agreed" metric set and the health semantics are the subagent's proposal; lives in core rather than an opt-in crate because the counters and tick boundaries are internal. Out of scope, not fixed: `/kick` sends Disconnect but never closes the connection server-side. Conflicts expected with #132/#138 in app.rs and config.rs. |
| VMC-ATP-033 | Advertise commands and complete command suggestions | Secondary function | 003 | Done | Implemented | [#163](https://github.com/VoidMinecraft/VoidMC/pull/163) | Yes (27 of 37, 16:44 UTC) | Tests + docs only, CI green. Five e2e scenarios: the tree a client receives is well formed and complete (14 default commands + 3 aliases, right parsers, ranges, executable flags, `/summon` flags); command-name completion works from the tree; server-side completion for `ask_server` arguments returns only valid matches with UTF-16 ranges and follows players joining and leaving; hostile and flooding requests (32,500-character lines, control characters, 500 requests in a burst) get the documented reply or none, with no error and no kick; a not-yet-ready player is answered. Who completes what: a vanilla client completes names, flags, `/tell`/`/kick` players and `/summon` entities by itself; the server only answers for custom `ask_server` arguments (no default command has one). Docs fixed: the tree is sent on entering Play, not "during configuration". Judgment calls: no reply to an unknown or bare command name counts as controlled; suggestions are not sorted (documented). Out of scope, not fixed: the server never sends the op-level status to clients, so a real client may not offer selectors like `@s`; a request with more than 32 KiB of multibyte text is logged at ERROR. |
| VMC-ATP-034 | Add custom player behavior through a Bevy plugin | Secondary function | 023 | In progress | Partial | [#151](https://github.com/VoidMinecraft/VoidMC/pull/151) | Yes (29 of 37, 17:27 UTC) | Fix + tests, CI green. The normal Join → Ready → Quit path worked with public API only. Five defects fixed: (1) a repeated PlayerLoaded fired PlayerReadyEvent twice; (2) a player who joined and left before PlayerLoaded got a join and never a quit; (3) no quit event at server shutdown; (4) `/kick` never closed the connection, so a client ignoring Disconnect stayed in the game; (5) an observer despawning a player entity panicked the server on that client's next packet. New public `Players::kick(entity, reason)`; kicks now flush the reason then close (2 s bound). Five e2e scenarios with a recording plugin + six unit tests. BEHAVIOUR CHANGES to review: PlayerQuitEvent now pairs with PlayerJoinEvent (fires for joined-but-never-ready players; check `Has<PlayerReady>` for the old meaning; the demo's quit observer was adapted); quit now fires for every player at shutdown; despawning a client entity closes its connection but fires no quit (documented as unsupported, use `Players::kick`). Out of scope, not fixed: keep-alive timeout only covers ready players. Conflicts expected with #145 (network.rs, server.rs). |
| VMC-ATP-035 | Observe gameplay events through a Bevy plugin | Secondary function | 023 | In progress | Partial | [#150](https://github.com/VoidMinecraft/VoidMC/pull/150) | Yes (22 of 37, 15:42 UTC) | Fix + tests, CI green. Move, rotate, chat, block place/break and item-use events were correct. Fixed: `PlayerCommandAction` used pre-1.21.2 numbering, so a real client's sprint fired `PlayerSneakEvent{false}` and sprint events never fired; now protocol-775 numbering, sneak events come from Player Input and sprint events from Player Command (`StartSneaking`/`StopSneaking` variants removed from voidmc-protocol: breaking, unused in the repo). Four e2e scenarios with a recording plugin for 22 event types that assert on the whole event log, with two players. events.md now states firing rules and that events are client input, not gameplay (no combat/damage). Assumption: a packet repeating the current position still fires a move event (existing behaviour, now documented). Out of scope, NOT fixed: `Interact` is decoded with the pre-26.1 layout and the separate `Attack` packet is not decoded, so real attacks produce no event and an off-hand entity click reads as an attack (sources disagree on the wire layout); placement does not check the target is replaceable. |
| VMC-ATP-036 | Provide a custom world generator | Secondary function | 023 | In progress | Partial | [#148](https://github.com/VoidMinecraft/VoidMC/pull/148) | Yes (20 of 37, 15:20 UTC) | Fix + tests, CI green. The configured generator was already used everywhere (spawn area, streaming, spawn height, loader fallback). Bug fixed: a section with more than 256 distinct block states was encoded with a palette list at 9+ bits, which a client cannot decode (Azalea panicked); such sections are now sent as raw global ids. Three e2e scenarios with a recording custom generator (world-coordinate pattern, column-varying surface, non-origin spawn (37.5, -21.5) → surface of column (37, -22); explicit spawn height wins; the documented example runs). `generate_spawn_area` (always default generator, unused by the server) is deprecated; `generate_chunk`/`surface_height_at` documented as default-only. Docs example fixed. Out of scope, not fixed: `ChunkSection::set_block_state` (block edits) still caps its palette at 8 bits, so a 257th distinct state in a section silently becomes another block. May conflict with #149 in void-protocol chunk.rs. |
| VMC-ATP-037 | Add a custom biome | Secondary function | 023 | In progress | Partial | [#149](https://github.com/VoidMinecraft/VoidMC/pull/149) | Yes (21 of 37, 15:31 UTC) | Fix + tests, CI green. A custom biome could already be registered, placed (generator or `set_biome`) and synced with public API only. Fixed: (1) registering after clients had the registries was only a debug assertion, so in release builds it silently desynchronized clients; now `BiomeError::RegistriesAlreadySent`; (2) malformed identifiers reached the registry packet; now `BiomeError::InvalidName`; (3) a generator-placed custom biome with id ≥ 128 in a section with more than 8 biomes was masked to biome 0. Example `/biome` gains a radius so the effect is visible (no automated test for the example change). Three e2e scenarios incl. a registry that crosses the 7→8-bit palette boundary, decoded `ChunksBiomes`, leave/re-enter, rejoin and late joiner. Assumption: required biome fields inferred from vanilla data (no 26.1.2 jar). Harness: adds `registry_data` (same lines as #146) and `tags` to JoinTranscript. Out of scope, not fixed: `RegistryDataStore::add_entry`/`remove_entry` and the public `registries` field can still change registries after login. |

## Run notes (verification run of 2026-11-19)

Result: 37 objectives checked, 37 PRs open (#127–#163), none merged, `ci-ok` green on all. 11 Implemented (tests only), 22 Partial (fix + tests), 4 Missing (implemented), 0 Blocked.

**Stacked PRs (merge the base first):** #136 (004) on #135 (027); #147 (030) on #134 (005); #154 (012) on #151 (034); #160 (022) on #137 (006).

**Merging.** Trial merge of all PR branches onto `main` in number order: 28 merge cleanly apart from each PR's one `mod` line in `void-e2e/tests/scenarios/main.rs`; the first 27 of those (all but #163) were also tested together: root `cargo test --workspace --all-features` and the full e2e suite (113 scenarios) pass. Nine need manual conflict resolution and were not tested in combination: #138 (`void/Cargo.toml`, e2e `Cargo.lock`), #139 (`plugins/configuration.rs`, `systems/chunk.rs`, `world.md`), #145 (`void-example/src/main.rs`), #151 and #154 (`plugins/play.rs`, `events.md`), #152 (`serverbound/play.rs`, `packet_ids.rs`, `network.rs`), #156 (`commands.md`), #159 (`plugins/item_use.rs`), #162 (`void-example/src/main.rs`). Known points to keep when resolving: #158 and #159 both change the creative-slot handler (slot -1 drops the stack; packet honoured on Creative servers only); after #138 or #145 merges, `void/tests/server_config.rs` from #161 needs the new config field added to its list.

**Behaviour changes to review** (beyond plain bug fixes): example MOTD changed to the ATP's text (#130); PlayerQuitEvent now pairs with PlayerJoinEvent and fires at shutdown (#151); players' glowing/invisible flags are now sent to other players (#134); cached chunks wait behind nearer ungenerated ones (#139); illegal chat and `§` in `/tell`/`/broadcast` are refused rather than kicked (#152, #154); item drops are thrown forward with a 40-tick pickup delay (#158); correct inventory predictions are no longer echoed (#159); opt-in additions: `ExecutionMode` (#138), `RedstonePlugin` (#147), observability endpoint (#145), `voidmc-load` runner (#142), `VOID_LISTEN_ADDR` for the example (#162).

**Decisions left open:** total order for the 26 unordered conflicting system pairs under `ExecutionMode::MultiThreaded` (#138); permission checks on default commands (anyone can `/kick`); whether `max_players` should be enforced (documented as advisory in #161); thresholds of the load workloads (#142) and the metric set (#145) were chosen by the subagents.

**Bugs found outside an objective's scope and NOT fixed:** `Interact` decoded with the pre-26.1 layout and `Attack` not decoded; status-only movement packet not decoded; `ChunkSection::set_block_state` caps its palette at 8 bits (a 257th block state in a section becomes another block); public `EntityBuilder::wander(true)` does nothing; no `SetEquipment` is ever sent (held items and armor are invisible to others); default placement overwrites a solid neighbour; hand placement ignores orientation; keep-alive timeout only covers ready players; `RegistryDataStore` can still be mutated after login; `VoidServer::run` drops the exit code; spawn settling only checks the centre column; an entity with gravity but no block checks falls forever; the op-level status is never sent to clients.

**Issues filed** for the open decisions on command permissions (#164) and `max_players` (#165) and for the unfixed bugs (#166–#191), each naming the objective and PR where it was found.

**Branch names.** 21 PR branches are literally named `__PK_ENTROPY_<12 hex>__` instead of `atp/NNN-…` (#135, #139, #140, #142–#148, #150–#152, #154–#156, #158, #159, #161–#163): a redaction layer in the coordinating session replaced the names written in subagent briefs, and the subagents used the placeholders as given. PR titles, contents and CI are unaffected. Renaming a head branch on GitHub closes its PR (tried on #162, then renamed back and reopened), so they were left as they are.

**Housekeeping.** The shared target grew by path-specific artifacts per worktree; 8.2 GB of artifacts from already-removed worktrees were deleted once (after the go-ahead to clean stale cache), nothing older than the run and no dependency cache. Subagents built with incremental compilation and debug info off to limit growth. All agent worktrees are removed; local branches from the run remain.

## Merge run (started 2026-11-19)

### Result

**All 37 ATP PRs are merged into `main`.** 36 went in during the unattended run; #159 (VMC-ATP-014) was skipped under the stop rule, and merged at 21:46 UTC after the owner chose how to resolve it. The last ATP merge is `8db6967` (#161); CI on that commit is green on every job, including the push-only `coverage` job. `main` then moved on with the flaky-test fixes described below. The only open PR is #17 (not touched). Four PRs outside the 37 were also merged: #192 and #193 during the run, #194 and #195 afterwards (see below).

**Merge order** (all merge commits, branch deleted, `ci-ok` green and branch up to date at each merge): #135, #129, #136, #134, #147, #137, #160, #127, #128, #130, #131, #132, #133, #140, #141, #142, #143, #144, #146, #148, #149, #150, #153, #155, #157, #158, #163, #139, #151, #154, #152, #156, #138, #145, #162, #161, #159. Constraints kept: each stack's base first; #158 before #159; #161 after #138 and #145.

**PRs outside the 37 that were merged during the run:** #192 (test-only fix for a timing-dependent worldedit test that turned `main`'s `coverage` job red after the first merge; merged under the stop rule) and #193 (the Benchmarks workflow no longer runs on pull requests; requested by the user during the run and merged on their explicit go-ahead).

**Final check on the final `main` (`4293b1a`, after #194, #159 and #195), run through a subagent: all eight checks pass.** fmt and clippy clean in both workspaces; lock files consistent (`--locked` builds, nothing to change); root `cargo test --workspace --all-features` 1429 passed, 0 failed, 1 ignored doctest, twice; full e2e suite 168 of 168 scenarios in each of three runs (plus 21 harness unit tests and 1 doctest). Scenario modules 002–037 are all present, 36 of them, each running at least one test; objective 001 is the crate-level startup test. The three tests that had been intermittent passed 60 of 60 single runs, half under CPU load. CI on `4293b1a` is green on every job including `coverage`, as are the three push runs before it. The coordinator re-read the CI result and the three e2e result lines itself.

**Earlier check at `8db6967`, before the two flaky-test fixes and #159 (kept for the record):** fmt and clippy clean in both workspaces; lock files consistent (`--locked` builds, nothing to change); root `cargo test --workspace --all-features` 1425 passed, 0 failed, twice; e2e suite 155 passed and 1 failed in the first run, 156 of 156 in the second. Scenario modules present: 002–013 and 015–037 (001 is the crate-level startup test; 014 is #159), each running at least one test.

**One intermittent test on `main`.** `atp_025_tps_diagnostics::an_unusable_tps_path_is_a_warning_and_the_server_runs_on` (from #162) failed once in seven executions, only inside a full run on a loaded machine: the server thread panics with "cannot inspect PostUpdate … `Schedule::initialize` needs to be called". It runs in the required `ci-ok` check, so it can fail CI now and then. **Fixed by #194, merged at 21:15 UTC on the user's explicit go-ahead** ("you can merge the flaky test fixes prs"). Cause confirmed by forcing the race: the e2e harness reported "ready" before collecting the first tick's queued jobs, so a world closure sent right after start could run during tick 1, before `PostUpdate` had run once. #194 reorders two lines in the harness (jobs sent after start now run from tick 2), adds a harness test and documents the guarantee; a second commit sets `slow_tick_ms(0)` in that one scenario, because on a loaded machine a slow-tick warning was counted as the warning the scenario looks for (1 failure in 20 runs under load). The two affected tests passed 35 of 35 runs (20 under load); `ci-ok` is green. It was first left unmerged, because it is not one of the 37 and no rule of the goal covered it; the user then approved it.

**A second intermittent test on `main`, found while fixing the first.** `atp_032_observability::kicks_despawns_and_shutdown_bring_the_players_gauge_back_to_zero` times out "waiting for Eve's connection closed": 1 failure in 15 runs alone on `main`. It is the scenario added in #145's follow-up commit during this run; four extra runs at the time all passed, which was too few to catch it. **Fixed by #195, merged at 21:58 UTC under the same go-ahead.** It was a real server bug, in the kick counter, not in connection handling: when game code despawns a connected player, the connection is always closed (within microseconds, measured in failing runs too), but the server dropped the player's sender before asking for the kick, so the "connection ended" message could reach the network loop first; the kick then found nothing, `voidmc_clients_kicked_total` was not incremented and "Kicked client" was not logged. A server whose game code despawns players under-reported that metric at random. #195 asks for the kick before dropping the sender (`network.rs`) and makes the network loop handle queued kicks before forgetting an ended connection (`server.rs`; the kick code itself moved into a function unchanged), with a unit test that failed 8 of 8 without the change. The scenario failed 7 of 90 runs before and 0 of 90 after (60 under load). This PR changes server code, which the coordinator pointed out to the user before merging. Left, not fixed: a theoretical window for `/kick` and keep-alive kicks if the game thread stalls for a full network round trip between queueing the disconnect and requesting the kick; and that scenario holds its shutdown latch while it panics, which adds a misleading "did not stop within 10s" to any failure.

**#159 was skipped, then merged.** It and #158 pinned opposite behaviour for a correctly predicted throw click (echo the slot, or not). The owner chose to keep #159's no-echo behaviour and change #158's two assertions; see row 37 of the merge log. `main` no longer accepts the creative-slot packet on a Survival server.

**Decisions waiting for the maintainers** (each written on the PR named):
- #151: at exit, the disconnect system and the persistence plugin's final flush run in `Last` with no order between them.
- #138: the order of the unordered conflicting system pairs under `ExecutionMode::MultiThreaded` (now 23 by default, 32 with `RedstonePlugin`, plus one in `Last` and 64 undocumented ones in `PostUpdate`).
- #145: whether operator kicks and plugin despawns belong in `voidmc_clients_kicked_total` (they are counted since #151).
- #152 and #154: two separate helpers refuse the same chat characters; unify them or not.
- Still open from the verification run: permission checks on default commands (#164), `max_players` enforcement (#165), the load thresholds (#142).

**Changes made beyond plain conflict resolution** (each in its own commit, described on its PR; no assertion of any scenario or unit test was changed anywhere):
- #160: docs sentence about the empty hand corrected for #147's block interaction.
- #155: docs section `/gamemode` moved below the `/summon` notes and one word respelled. This fixed a defect that was already in the PR, so it was unrequested; revert that commit to undo.
- #151: `on_player_ready` uses `try_insert` so a player despawned by another observer no longer panics the game thread (the merge exposed it); a unit test covers both observer orders.
- #156: docs row for `PlayerMoveEvent` gains the unconfirmed-teleport case.
- #139: one sentence of #135's docs reworded for the capped join radius; one line of new code (`main`'s join structure built from #139's radius).
- #138: docs example replaced (made false by #158); #142's load runner reads its two CPU figures in one pass (its check failed 3 runs in 5 after the merge).
- #145: docs row and help text of the kick counter updated for #151; a scenario added that pins the counter and the players gauge.
- #162: the example reads the environment in one way; #129's startup test clears `VOID_LISTEN_ADDR`; a unit test pins the address defaults; one docs row.
- #161: its field list gained `execution_mode` and `observability`.

**Worth knowing.** Until #138, the e2e build ran Bevy's multi-threaded executor by accident (feature unification through the client crates). It now runs single-threaded, like the default server; only `atp_028` asks for the multi-threaded one.

**Issues filed for these on 2026-11-19, on the user's go-ahead:** #196 (load report thread name), #197 (`time_per_tick` overflow), #198 (movement packet during an unconfirmed teleport), #199 (`stopping` before the last gauges), #200 (kick uncounted if the client closes first; unverified, `needs-info`), #201 (five docs passages), #202 (test coverage gaps), #203 (two fragile tests). The decisions listed above were not filed as issues.

**Defects found during the merges and NOT fixed** (already on `main`, none caused by a merge):
- #142's load report: the "server threads" list names `tokio-runtime-w`, but the locked tokio 1.53.1 names its workers `tokio-rt-worker`, so the network runtime's CPU has never been counted.
- `WorldEditPlugin::time_per_tick` panics on a very large duration (overflow when added to the current time).
- #156: while a teleport is unconfirmed, the on-ground bit and rotation of a movement packet are still applied.
- Docs: `players.md` step 6 names `initial_chunk_radius` without the view-distance cap; `benchmarking.md` describes the CSV column `last_tick_ms` as a tick duration, while it is the time between tick ends; the `ServerConfigResource` paragraph of `configuration.md` does not say `execution_mode` and `observability` are not mirrored; #138's docs do not mention the `PostUpdate` and `Last` pairs.
- #145: the health state switches to `stopping` just before the gauges are stored. #129's startup test binds the fixed port 25565.
- Not covered by any test: observability together with `MultiThreaded`; #144's whole-fall landing check in multi-threaded mode; the example launched with the observability and diagnostics variables together.

**Cleanup.** All agent worktrees are removed. The local branches of the merged PRs were deleted at merge time; the leftover local branches of this session's agent worktrees were deleted with git's safe delete (none refused). Kept: `main` and the user's `pr-86`. Stale remote-tracking refs were pruned. The locally merged `item_use.rs` for #159 and a diff are in `.claude/atp-merge-run/` (untracked). No issue was created or changed during the merge run itself; #196–#203 were created afterwards.

### Plan and calls made during the run

Goal: merge PRs #127–#163 into `main` one at a time (merge commits, branch deleted, `ci-ok` green and branch up to date before each merge).

**Planned order.** Stacks first (#135, #136; #134, #147; #137, #160), then the PRs with no known conflict (#129, #127, #128, #130–#133, #140–#144, #146, #148–#150, #153, #155, #157, #158, #163), then the ones with known conflicts (#139, #151, #154, #152, #156, #159, #138, #145, #162) and last #161 (it must follow #138 and #145).

**Calls made.**
- Every PR except #129 adds its `mod atp_NNN_…;` line at the same place in `void-e2e/tests/scenarios/main.rs`, so after the first merge every PR conflicts with `main` on at least that line. `gh pr update-branch` is used only where a local trial merge shows no conflict; every other PR goes to its own subagent, which merges `origin/main`, resolves, runs fmt, clippy, the workspace tests and the e2e suite, pushes and waits for CI.
- A stacked PR is retargeted to `main` by hand just before its base is merged (same end state as GitHub's automatic retarget, without depending on it).
- Before each merge the coordinator checks three things by script: the PR head contains `main`'s tip, `ci-ok` is green on that head, and the CI run of `main`'s tip is green.

**Incidents.**
- After merge 1 (#135) the CI run on `main` went red in the `coverage` job, which only runs on pushes to `main`: `void-worldedit/tests/engine.rs::tall_sparse_edits_resend_the_chunk_once` got `(1, 7)` instead of `(1, 0)`. The same commit passed the `test` job, and a rerun of `coverage` passed, so it is a timing flake (the worldedit engine has a 15 ms wall-clock budget per tick), not something #135 changed (#135 does not touch worldedit). Following the stop rule, merging was paused and the test was given to a subagent to fix in its own PR.

**Merge log.**

| # | PR | Objective | Merged (UTC) | How it was updated | Conflicts resolved / notes |
|---|---|---|---|---|---|
| 1 | #135 | 027 | 12:11 | already up to date | none |
| – | #192 | (main CI fix) | 12:33 | already up to date | Not an ATP PR. Test-only fix for the coverage incident: the worldedit engine tests no longer use the 15 ms wall-clock budget, so only the block budget splits edits. Cause confirmed by the subagent by injecting a stall (reproduces `(1, 7)` exactly). Merged under the stop rule before any further ATP merge. |
| 2 | #129 | 001 | 12:41 | `gh pr update-branch` (twice: once after #135, once after #192) | none (it does not touch the e2e module list) |
| – | #193 | (CI change) | 12:44 | `gh pr update-branch` | Not an ATP PR. Requested by the user during the run and merged on their explicit go-ahead: the Benchmarks workflow no longer runs on pull requests (weekly schedule and manual trigger kept). |
| 3 | #136 | 004 | 12:48 | retargeted to `main` before #135 merged, then `gh pr update-branch` | none (its branch already held #135's module-list line) |
| 4 | #134 | 005 | 12:57 | subagent merged `main` (merge commit, normal push) | Only the module-list line in `void-e2e/tests/scenarios/main.rs` (other side: #136); all lines kept, sorted. No semantic conflict; fmt, clippy, workspace tests (1301 passed) and e2e (23 scenarios) passed locally and in CI. Coordinator checked that the merge commit changed none of the PR's own 14 files except that list. |
| 5 | #147 | 030 | 13:05 | retargeted to `main` before #134 merged; subagent merged `main` | Only the module-list line (other side: #136, #134); all lines kept, sorted. `void-protocol/src/clientbound/play.rs` and `void-protocol/tests/packet_ids.rs` merged automatically (coordinator checked the added lines are exactly #136's). Workspace tests (1319) and e2e (30) passed locally and in CI. |
| 6 | #137 | 006 | 13:14 | subagent merged `main` | Two conflicts: the module-list line, and `void-worldedit/tests/engine.rs` (other side: #192). Kept both: #137's rewritten wand test as written, with its server built through #192's no-time-limit `plugin()` helper (all 17 `Server::new` calls use it). `item_behavior.rs`, `item_use.rs` and `items.md` merged automatically with #147's redstone changes; the subagent read the merged `drain_item_use_queue` (lever interaction first, then placement; the empty-hand guard kept). Coordinator checked the pushed merge differs from git's automatic merge only in the two conflicted files. Workspace tests (1321) and e2e (36) passed. Retargeting #160 with `gh pr edit` failed three times with a GitHub server error; the REST API call worked. |
| 7 | #160 | 022 | 13:23 | retargeted to `main` before #137 merged; subagent merged `main` | Only the module-list line. One follow-up commit, docs only: #160's sentence "An empty hand runs nothing and places nothing" was no longer true with #147 on `main` (an empty hand still runs the clicked block's interaction, e.g. toggles a lever; tested in `redstone.rs`), so it was reworded. No behaviour changed; coordinator checked the merged order in `drain_item_use_queue` (item behaviour, block interaction, default placement skipped for an empty hand) and noted the change on the PR. The coordinator's brief had the first two steps in the wrong order; the subagent followed the code, not the brief. Workspace tests (1323) and e2e (44) passed. |
| 8 | #127 | 020 | 13:32 | subagent merged `main` | Only the module-list line. Its two world-loader scenarios pass against #135/#136's new chunk sending. `void-e2e/Cargo.toml` and `void-e2e/Cargo.lock` merged automatically; the lock differs from `main` only by the PR's own `voidmc-world-io` dev-dependency (checked). Workspace tests (1323) and e2e (46) passed. |
| 9 | #128 | 023 | 13:39 | subagent merged `main` | Only the module-list line; no semantic conflict. Workspace tests (1323) and e2e (47) passed. |
| 10 | #130 | 002 | 13:47 | subagent merged `main` | Only the module-list line. `void-example/src/main.rs` merged automatically with #147's change. #129's startup test and #130's MOTD/config change agree (the startup test does not check the MOTD); both ran and passed. Workspace tests (1324) and e2e (49) passed. The subagent left a stuck wait loop behind (a `pgrep -f` loop that matched itself); the coordinator stopped it and added a rule against that pattern to the subagent rules. Note for later: #129's startup test binds the fixed port 25565, so two runs of the example's tests on one machine at the same time can collide. |
| 11 | #131 | 003 | 13:56 | subagent merged `main` | Only the module-list line. The pinned join trace still matches with #136's chunk-batch framing (the scenario skips unknown packets). Noted on the PR: the scenario never sends `ChunkBatchReceived`, which is fine only while the server does not wait for that acknowledgement. Workspace tests (1324) and e2e (50) passed. |
| 12 | #132 | 026 | 14:04 | subagent merged `main` | Only the module-list line. `void-e2e/Cargo.toml` is now identical to `main` (#127 and #132 added the same `voidmc-world-io` dev-dependency; it appears once). Lock files merged automatically and differ from `main` only by this PR's `ctrlc` and `tracing-subscriber` entries (checked); every cargo run used `--locked`. All scenarios on `main` pass on the PR's changed test harness (`void-e2e/src/server.rs`). Workspace tests (1328) and e2e (51) passed. |
| 13 | #133 | 021 | 14:11 | subagent merged `main` | Only the module-list line. `commands.md` merged automatically (#133 edits the `CommandBuilder` example, #132 the `/stop` paragraph; far apart, no contradiction). Workspace tests (1328) and e2e (53) passed. |
| 14 | #140 | 009 | 14:19 | subagent merged `main` | Only the module-list line. Its e2e manifest and lock file are now identical to `main` (same `voidmc-world-io` dev-dependency as #127); `cargo test --no-run --locked` works. The three restart scenarios pass against #132's stop and save-on-stop changes. Workspace tests (1328) and e2e (56) passed. |
| 15 | #141 | 016 | 14:27 | subagent merged `main` | Only the module-list line. Its summon scenarios (including the check that a summoned entity gets no stray move or metadata packets for 40 ticks) pass against #134's metadata and movement changes. Workspace tests (1328) and e2e (58) passed. |
| 16 | #142 | 031 | 14:36 | subagent merged `main` | Only the module-list line. e2e manifest and lock differ from `main` only by this PR's `serde`/`serde_json` (checked; `--locked` build works). No threshold was touched: after #132/#135/#136 the smoke workload measured 4/4 clients, 19.97 TPS (limit 10), round-trip p95 50 ms (limit 2000 ms), 0 errors. Workspace tests (1328) and e2e (66 scenarios, all 8 `atp_031`) passed. The subagent overwrote three old scratch files of an earlier subagent in the shared scratchpad (`smoke.json`, `smoke.md`, `e2e1.log`; not deliverables); a unique-name rule was added to the subagent rules. |
| 17 | #143 | 017 | 14:46 | subagent merged `main` | Only the module-list line. Both wander scenarios pass against #134's movement changes (also 4 extra runs alone, no flake). Workspace tests (1328) and e2e (68) passed. |
| 18 | #144 | 018 | 14:59 | subagent merged `main` | Only the module-list line. Checked both ways against #141/#143: the physics change only runs for entities with gravity enabled, and the summon and wander scenarios do not pass `--gravity`, so they never reach it. `atp_016`, `atp_017`, `atp_018` passed in the full suite and in 4 extra runs. Workspace tests (all dependent crates ran) and e2e (72) passed. Note for later: summoning with gravity on by default would bring #141's "no stray move packets for 40 ticks" check into contact with this code. |
| 19 | #146 | 029 | 15:09 | subagent merged `main` | Only the module-list line. Its three lines in the shared client `void-e2e/src/bot.rs` build with #142's load runner; both biome/weather scenarios pass with #135/#136's chunk sending. Workspace tests (1336) and e2e (74) passed. |
| 20 | #148 | 036 | 15:20 | subagent merged `main` | Only the module-list line. `world.md` merged automatically with #135/#136's text; the subagent read the whole page (coherent, nothing contradicts; the documented `init_world` signature matches the code). The chunk scenarios already on `main` (`atp_004`, `atp_020`, `atp_027`, `atp_029`) and `atp_036` all ran and passed with #148's chunk encoding and generator changes. Workspace tests (1337) and e2e (77) passed. |
| 21 | #149 | 037 | 15:31 | subagent merged `main` | Only the module-list line. Three files changed by both sides merged automatically and were read: `chunk.rs` (#148 widens block-state palettes, #149 biome palettes; different containers, each helper once, both unit tests pass), `void-e2e/src/bot.rs` (#146 and #149 added the same `registry_data` field; it appears once, checked) and `biomes.md` (both texts present, no contradiction). `atp_004`, `atp_027`, `atp_029`, `atp_036`, `atp_037` ran and passed. Workspace tests (about 1339) and e2e (80) passed. |
| 22 | #150 | 035 | 15:42 | subagent merged `main` | Only the module-list line. Sneaking traced in the merged code: #134 sets `Sneaking` from the Player Input sneak bit, and #150 raises `PlayerSneakEvent` from the same bit of the same packet, each only on change, so they agree (one state change, one event). #150 removes `PlayerCommandAction::StartSneaking`/`StopSneaking`; no other code and no remaining PR uses them (checked). `atp_005`, `atp_006`, `atp_022`, `atp_030`, `atp_035` ran and passed. Workspace tests (1344) and e2e (84) passed. |
| 23 | #153 | 008 | 15:54 | subagent merged `main` | Only the module-list line. The pinned `/help` text still matches (no merged PR changed a default command). `commands.md` merged automatically with #132's and #133's edits; the whole page was read, no contradiction. `atp_008`, `atp_016`, `atp_021`, `atp_026` ran and passed. Workspace tests (1344) and e2e (92) passed. Note for later PRs: `/plugins` without a plugin list now answers "No plugins loaded." and a blank command answers "Unknown command: /"; no remaining PR pins the old text (checked). |
| 24 | #155 | 010 | 16:06 | subagent merged `main` | Only the module-list line. `commands.md` (now edited by #132, #133, #153, #155) and `configuration.md` merged automatically and were read. **One commit beyond the merge, docs only:** #155 had put its `/gamemode` section between the `/stop` and `/summon` notes, so the `/summon` text read as part of it; the subagent swapped them and changed "grey" to "gray". This predates the merge, so it is an unrequested fix, not a conflict resolution; the coordinator checked the diff is a pure move plus that word and noted it on the PR (revert that commit to undo). No description string touched; `atp_010` and `atp_008` pass together. Workspace tests (1344) and e2e (95) passed. |
| 25 | #157 | 013 | 16:18 | subagent merged `main` | Two conflicts: the module-list line, and `commands.md` (other side: #155's `/gamemode` section in the same spot). Kept `main`'s page unchanged and added #157's 7-line `/give`/`/clear` note after the `/summon` notes, before `### /gamemode` (coordinator checked: +7 lines versus `main`, nothing removed). `defaults.rs` merged automatically; #157 only adds the refusal of `minecraft:air`, no `/help` text changes, `atp_008` passes unchanged. `atp_006`, `atp_008`, `atp_010`, `atp_013`, `atp_022` ran and passed. Workspace tests (1344) and e2e (101) passed. |
| 26 | #158 | 015 | 16:32 | subagent merged `main` | Only the module-list line. Five overlaps with merged PRs merged automatically and were read: `item_use.rs` (#137/#160/#147's handlers intact; #158's change is only in `handle_creative_slot`), the packet registry (`TakeItemEntity` at 0x7C, registered once; packet-id test passes), `events.rs` (a doc comment only), `items.md` (all PRs' text present, no contradiction) and physics (#144; drop slowdown runs after the physics step). `atp_006`, `atp_013`, `atp_015`, `atp_018`, `atp_022`, `atp_030`, `atp_035` ran and passed; `atp_015` and `atp_018` passed 4 extra runs each. Workspace tests (1348) and e2e (106) passed. Reported, not changed (already in the PR): the creative-slot packet is accepted in any game mode, which #159 addresses. |
| 27 | #163 | 033 | 16:44 | subagent merged `main` | Only the module-list line. `commands.md` merged automatically: versus `main` only #163's tab-completion hunks differ (+21/-2), so the text of #132, #133, #153, #155, #157 is unchanged. Its pinned command tree and suggestions match `main`'s defaults after #153 and #157. `atp_003`, `atp_008`, `atp_010`, `atp_013`, `atp_021`, `atp_033` ran and passed. Workspace tests (1348) and e2e (111) passed. (The subagent's report gave 7 tests for `atp_008`; the file has 8 and is identical to `main`'s, so that was a miscount, not a dropped test.) |
| 28 | #139 | 019 | 17:00 | subagent merged `main` | Four conflicts, smaller than expected. `chunk.rs`: production code merged automatically (#136's batch framing and #139's hold-back are on different lines); the conflict was two unit tests added at the same place, both kept. `configuration.rs`: one line, `main`'s join structure (#135/#136) built from #139's capped join radius — the only code neither side had. `world.md`: both bullets kept, and one sentence of #135's reworded because the join radius is now capped by the view distance. Module list. No product decision: the backlog holds the unsent tail in the same nearest-first order. No scenario assertion changed. `atp_019`, `atp_004`, `atp_027`, `atp_031` passed 3 extra runs each; all chunk scenarios passed. e2e 118. Coordinator read the hand-resolved lines and noted the resolution on the PR. Left: `players.md` step 6 names `initial_chunk_radius` without the cap (incomplete, not wrong). |
| 29 | #151 | 034 | 17:27 | subagent merged `main` | Three conflicts, all small: the `use` block of `plugins/play.rs` (union of #151's and #136's imports), the import line of the first example in `events.md` (#151's kept; #150's tables intact), the module list. Eight more files changed on both sides merged automatically and were read. **One follow-up code fix the combination needed:** after the merge #151's own scenario about an observer that despawns the player panicked the game thread on every run, because observer order changed with the observers now on `main` and the built-in `on_player_ready` queued plain inserts on a despawned entity; they are now `try_insert`s (`void/src/systems/player.rs`, with a unit test for both orders). No scenario assertion changed. Lifecycle scenarios passed, 5 extra runs each of `atp_034`, `atp_031`, `atp_026`, `atp_002`. Workspace tests (1356) and e2e (123). Noted on the PR. **Left for a decision:** at exit, #151's disconnect system and world-io's final flush (#132) both run in `Last` with no order; a quit observer that edits blocks at shutdown could miss the last save (nothing does this today). |
| 30 | #154 | 012 | 17:41 | retargeted to `main` before #151 merged; subagent merged `main` | Only the module-list line (its earlier conflicts came from #151, now on `main`). `defaults.rs` and `commands.md` merged automatically and were read: #154's `§` refusal for `/tell` and `/broadcast` sits beside #151's `/kick`, #153's descriptions and #157's air refusal; no `/help` text changes, `atp_008` passes unchanged. `atp_002`, `atp_005`, `atp_008`, `atp_012`, `atp_013`, `atp_033`, `atp_034` ran and passed; `atp_012` and `atp_034` passed 4 extra runs each. Workspace tests (1356) and e2e (127). To check with #152: both should refuse `§` with the same wording and not add a second helper for the same check (#154's is `refuse_illegal_characters`). |
| 31 | #152 | 007 | 17:55 | subagent merged `main` | Four conflicts, all lists both sides added to: the serverbound packet registry and the packet-id test (#136's `ChunkBatchReceived`, #152's `ChatAck` and `ChatSessionUpdate`; each registered once), `network.rs` (`main`'s version with #151's lifecycle kept unchanged, #152's two dispatch arms added: +10 lines versus `main`, checked), the module list. No code neither side had, no follow-up commit. `chat.rs` merged automatically with #153: commands are still checked first, never refused by the chat check, never broadcast. The chat code never disconnects; disconnects still go through #151's path. Chat, command, lifecycle and load scenarios passed (`atp_007` 5 extra runs). Workspace tests (1361) and e2e (133). **Noted on the PR for a follow-up:** two separate helpers now refuse the same characters with the same message (#154's in `defaults.rs`, #152's in `chat.rs`); not unified here. |
| 32 | #156 | 011 | 18:10 | subagent merged `main` | Two conflicts: the module list, and one line of `commands.md` (the `/tp` row of the table #153 rewrote: `main`'s row kept, #156's arguments note added; description string unchanged). **One follow-up commit, docs only, needed by #150 + #156:** #150's `PlayerMoveEvent` row now also says the event is not fired while a teleport is unconfirmed (checked against the merged `apply_position`). `plugins/movement.rs` merged automatically: #156's pending-teleport guard beside #134's checks; #134's on-ground and sneaking handling untouched. `atp_011`, `atp_005`, `atp_019` passed 3 extra runs each. Workspace tests (1362) and e2e (137). Noted on the PR, already in #156: while a teleport is unconfirmed the on-ground bit and rotation of a movement packet are still applied (vanilla ignores the whole packet). |
| – | #159 | 014 | (first attempt, 18:30; not merged then) | subagent merged `main` locally, then aborted; nothing pushed | **Skipped at that point under the stop rule (product decision); merged later, see row 37.** #159 and #158 pin opposite behaviour for a correctly predicted throw click in the player inventory: #158's scenario on `main` (`atp_015`, `every_drop_path_throws_exactly_the_dropped_stack`) waits for the server to send the slot back twice; #159 deliberately stops echoing correct predictions and `atp_014` fails if an update arrives. On the merged tree e2e gives 148 passed, 1 failed (that scenario, twice, deterministic). Resolving it means choosing the behaviour and changing the other PR's assertions. Options and a recommendation (keep #159's no-echo, change #158's two `expect_slot` calls to check the counts another way; objective 015 does not require a slot packet and #158's description defers click semantics to 014) are written on the PR. The rest of the merge is ready: the creative-slot handler combines both (Creative-only covers the whole packet, including #158's slot -1 drop); the locally merged `item_use.rs` and a diff are kept in `.claude/atp-merge-run/` (untracked). No PR depends on #159. Until it is merged `main` still accepts the creative-slot packet on a Survival server. |
| 33 | #138 | 028 | 19:06 | subagent merged `main` | Five conflicts: `physics.rs` (system code merged automatically; the conflict was both sides appending tests, all kept; versus `main` the file's diff is line for line #138's original, so #144's whole-fall landing check is untouched and both execution modes call it), `void/Cargo.toml` (`bevy_tasks` and #132's `ctrlc`), `world/mod.rs`, the e2e lock (`main`'s plus one line; `--locked` builds work in both workspaces), the module list. **Two follow-up commits:** (1) docs: the unordered-sets paragraph names `Redstone`, and its example, made false by #158's ordering, was replaced; no order chosen. (2) `fix(e2e)` in #142's load runner: after the merge `atp_031`'s check "server threads' CPU >= game thread's" failed 3 of 5 runs because the two figures were read at different moments as floats; they are now read in one pass in integer nanoseconds (assertion and what is counted unchanged; 8 of 8 afterwards). **Worth knowing:** the e2e build had been running Bevy's multi-threaded executor by accident (feature unification through Azalea); with #138 the e2e suite runs single-threaded like the default server, and all scenarios pass. **Unordered conflicting pairs** (measured, no test pins them): `Update` 26 on #138 alone, 23 on the merged tree (5 gone through #158's ordering, 2 new from #158's `slow_dropped_items`), 32 with `RedstonePlugin`; 1 new in `Last` (#151); 64 in `PostUpdate`, undocumented, same before and after. No order was chosen; the decision stays open. `atp_028`, `atp_018`, `atp_015`, `atp_017` passed 6 extra runs; unit tests 10. Workspace tests (1372), e2e (138). All of this is on the PR. **Reported, not fixed (already on `main` from #142, confirmed by the coordinator):** the load report's "server threads" list names `tokio-runtime-w`, but the locked tokio 1.53.1 calls its workers `tokio-rt-worker`, so the network runtime's CPU has never been counted. |
| 34 | #145 | 032 | 19:30 | subagent merged `main` | Three conflicts: the example's `main.rs` (`main`'s `server_config()`, its test and `RedstonePlugin` kept; #145's `VOID_OBSERVABILITY_ADDR` opt-in moved inside `server_config()`; endpoint off by default, #129's startup test passes), the imports of `server.rs` (against #151; kicks counted once, after #151's graceful kick), the module list. `network.rs`, `app.rs`, `config.rs`, `lib.rs` merged automatically and were read. **One follow-up commit:** with #151, `/kick` and plugin despawns really close the connection through the kick path, so `voidmc_clients_kicked_total` now counts them; #145's docs said `/kick` was not counted. No counting code changed: the docs row and help text were updated and a scenario added that pins the counter and that the players gauge returns to zero after a kick, a despawn and a stop (checked against a real regression). No metric added, removed or renamed. `atp_032`, `atp_034`, `atp_031` passed 4 extra runs each. Workspace tests (1396), e2e (146). On the PR: whether operator kicks belong in that counter is the maintainers' call; not tested: observability with `MultiThreaded`, the overflow kick with the metric. |
| 35 | #162 | 025 | 19:49 | subagent merged `main` | Two conflicts: the module list, and the example's `main.rs`, where four PRs meet. Merged version reads the environment in one way: `env_var` is the only reader, `server_config(&metrics_env, var)` holds #130's MOTD and limits, #162's `VOID_LISTEN_ADDR`, then #145's `VOID_OBSERVABILITY_ADDR` opt-in; #147's plugin and #162's diagnostics untouched. New code neither side had: the lookup parameter and shared trimming. One non-textual break fixed in the merge: `main` still called `metrics_env.enabled()`, which #162 removes. **Three follow-up commits (two test-only, one docs), no assertion changed:** #129's startup test clears `VOID_LISTEN_ADDR` so it tests the default address; a new unit test pins the defaults and overrides; `VOID_OBSERVABILITY_ADDR` added to the one variable table lacking it. All 16 example unit tests exist and pass; the two test binaries that launch the example cannot collide on a port (3 extra runs). #162's TPS figure and #145's metrics are independent and consistent. Workspace tests (1418), e2e (149). Noted on the PR: the CSV column `last_tick_ms` (time between tick ends) is described in `benchmarking.md` as a tick duration, easy to confuse with #145's work-time metric (already on `main`). |
| 36 | #161 | 024 | 20:05 | subagent merged `main` | Only the module-list line; `config.rs`, `configuration.md`, `architecture.md` merged automatically (struct, `Default` and builder agree for all 23 fields). **One test-only follow-up commit:** `void/tests/server_config.rs` destructures `ServerConfig` exhaustively, so it did not compile until `execution_mode` (#138) and `observability` (#145) were listed; each got a row (type and default checked against the reference table) and a builder call, like every other field; the destructuring stays exhaustive. No expectation of the seven `atp_024` scenarios had to change. Workspace tests (1425), e2e (156). On the PR, already on `main`: the `ServerConfigResource` paragraph does not say the two new fields are not mirrored. |
| – | #194 | (flaky-test fix) | 21:15 | already up to date | Not an ATP PR; merged on the user's explicit go-ahead. Harness: world closures sent after start run from the second tick; `slow_tick_ms(0)` in one `atp_025` scenario. See Result. |
| 37 | #159 | 014 | 21:46 | subagent merged `main` again, after the decision | **The owner chose option 1:** keep #159's no-echo behaviour and change #158's two assertions. Conflicts: the module list and `handle_creative_slot` (against #158). In `atp_015`, the two `expect_slot` calls after the Q and Ctrl+Q throw clicks now read the server's inventory (4 emeralds, then empty); nothing else in #158's scenario changed (coordinator checked: 2 lines removed, a helper and 2 assertions added; 7 other `expect_slot` calls untouched). The handler: a negative slot drops only on a Creative server and only within the item's maximum stack; otherwise #159's rule. Those two points go beyond both PRs, were put to the owner with the decision and went in as prepared; both are pinned by `atp_014`'s extended creative-slot scenario (each condition removed in turn made it fail). No other merged path relied on the echo. `atp_014` and `atp_015` passed 5 extra runs. Workspace tests (1428), e2e (168). Left, already in #159: the "Syncing" paragraph of `items.md` lists the click handlers among setters whose changes are sent. |
| – | #195 | (flaky-test fix, server code) | 21:58 | `gh pr update-branch` | Not an ATP PR; merged under the user's go-ahead for the flaky-test fixes. Kick requested before the sender is dropped; queued kicks handled before an ended connection is forgotten. See Result. |

## Details

### VMC-ATP-001: Start the example server

**Category:** Critical feature  
**ATP status:** Done

**Objective:** Confirm that the repository source builds and starts the supported example server.

**Prerequisites:**

- Repository at commit 795d8c3b9cc9acbd422a14b2fae977ef705f5fb8
- a Rust toolchain compatible with Rust 1.91.1
- TCP port 25565 is available
- write access to a temporary runtime directory.

**Steps:**

1. Open a terminal at the repository root.
2. Run cargo run -p voidmc-example.
3. Wait for startup to complete.
4. Observe the startup logs and listening address.

**Expected results:** The command completes the build phase without an error; the server remains running; the logs contain Server listening, listen_addr: 127.0.0.1:25565 and Generated 81 spawn area chunks; no fatal error or panic is reported.

### VMC-ATP-002: Display the server status

**Category:** Critical feature  
**ATP status:** Done

**Objective:** Confirm that a compatible status client receives the server discovery information.

**Prerequisites:**

- VMC-ATP-001 has passed acceptance
- a Minecraft Java Edition 26.1.2 status client or equivalent protocol probe
- access to 127.0.0.1:25565.

**Steps:**

1. Add or query the server at 127.0.0.1:25565.
2. Refresh the server status.
3. Inspect the advertised version, protocol, MOTD, and player capacity.

**Expected results:** The server answers without timing out; it advertises Minecraft 26.1.2, protocol 775, MOTD Welcome to Void Server!, maximum 100 players, and the current online-player count.

### VMC-ATP-003: Complete the implemented login and configuration handshake

**Category:** Critical feature  
**ATP status:** Done

**Objective:** Confirm that a compatible client completes the implemented Handshake, Login, Configuration, and Play transitions and becomes ready without claiming online authentication, encryption, or compression.

**Prerequisites:**

- VMC-ATP-001 has passed acceptance
- one Minecraft Java Edition 26.1.2 client with a usable player session
- access to 127.0.0.1:25565
- packet-debug logging or an equivalent packet trace is available.

**Steps:**

1. Start the example server with packet-debug logging enabled.
2. Connect a Minecraft Java Edition 26.1.2 client to 127.0.0.1:25565.
3. Capture Handshake, LoginStart, LoginSuccess, and LoginAcknowledged.
4. Capture KnownPacks, ClientInformation, RegistryData, UpdateTags, FinishConfiguration, and FinishConfigurationAcknowledged.
5. Capture Play Login, Commands, initial chunk packets, and SynchronizePlayerPosition.
6. Wait for PlayerLoaded and the ready play state.

**Expected results:** The client transitions from Handshake to Login, Configuration, and Play; LoginSuccess contains the supplied profile with empty properties; registry data and tags are delivered; the client receives the Play login, command tree, initial chunks, and position synchronization; PlayerLoaded leads to the ready state; no online authentication, encryption, or compression is claimed or required.

**ATP feedback:** Implemented in the current code. Keep the acceptance test pending until a reproducible end-to-end packet trace is retained.

### VMC-ATP-004: Load the initial world

**Category:** Critical feature  
**ATP status:** Done

**Objective:** Confirm that the connected player receives and renders the initial chunk area while moving locally.

**Prerequisites:**

- VMC-ATP-003 has passed acceptance
- the player is connected at the spawn location.

**Steps:**

1. Wait until the terrain around the player is visible.
2. Turn through 360 degrees and inspect nearby terrain.
3. Move at least 32 blocks from spawn.
4. Return to the initial area.

**Expected results:** Nearby chunks render without leaving the player permanently in an empty world; additional terrain appears as the player moves; movement remains responsive; returning to the initial area does not disconnect the client or corrupt the visible world.

### VMC-ATP-005: Synchronize two players

**Category:** Critical feature  
**ATP status:** Done

**Objective:** Confirm that two connected players can see each other's presence and movement in the same world.

**Prerequisites:**

- VMC-ATP-003 has passed acceptance for two distinct Minecraft Java Edition 26.1.2 client sessions
- both players are connected in the same area.

**Steps:**

1. Connect Player A and Player B to the same server.
2. Position both players within visible range.
3. Move, rotate, jump, and crouch with Player A.
4. Repeat with Player B.
5. Disconnect one player.

**Expected results:** Each player sees the other appear; movement and orientation updates are visible to the opposite client without a persistent duplicate or frozen entity; the disconnected player disappears from the remaining client's world.

### VMC-ATP-006: Modify shared blocks

**Category:** Critical feature  
**ATP status:** Done

**Objective:** Confirm that block breaking and placement update the authoritative world and are broadcast to nearby players.

**Prerequisites:**

- VMC-ATP-005 has passed acceptance
- both players are in Creative mode near the same target blocks.

**Steps:**

1. Break a visible block with Player A.
2. Confirm the result from both players.
3. Place a block with Player B in the cleared position.
4. Confirm the result from both players.
5. Repeat with a block outside the immediate view and return to it.

**Expected results:** A broken block disappears for both players; a placed block appears at the same position for both players; the shared world converges to one state; no invalid interaction crashes the server.

### VMC-ATP-007: Exchange chat messages

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that chat entered by one player is delivered to all connected players.

**Prerequisites:**

- VMC-ATP-005 has passed acceptance
- both players can use the in-game chat interface.

**Steps:**

1. Send a unique chat message from Player A.
2. Confirm receipt on Player A and Player B.
3. Send a different message from Player B.
4. Confirm receipt on both clients.

**Expected results:** Each submitted message is displayed to both connected players once; sender identity and message content remain recognizable; chat use does not disconnect either player or stop the server.

### VMC-ATP-008: Discover available commands

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that a connected player can discover representative commands and receive a controlled response for invalid input.

**Prerequisites:**

- VMC-ATP-003 has passed acceptance
- the connected player can open chat and enter commands.

**Steps:**

1. Run /help.
2. Run /ping.
3. Run /plugins.
4. Enter an unknown command.

**Expected results:** /help lists available commands; /ping returns a response; /plugins reports the loaded plugin list or an explicit empty-list response; the unknown command produces a controlled error or help response rather than a crash.

### VMC-ATP-009: Preserve a world change after restart

**Category:** Critical feature  
**ATP status:** Done

**Objective:** Confirm that an accepted world modification survives a controlled server restart.

**Prerequisites:**

- VMC-ATP-003 and VMC-ATP-006 have passed acceptance
- a dedicated writable test runtime directory
- permission to stop and restart the local server
- the coordinates of a distinctive test block are recorded.

**Steps:**

1. Place a distinctive block at a recorded coordinate.
2. Stop the server cleanly.
3. Restart the same build with the same world directory.
4. Reconnect and return to the recorded coordinate.

**Expected results:** The server stops and restarts without corrupting the world directory; the player can reconnect; the distinctive block remains at the recorded coordinate after restart.

### VMC-ATP-010: Change a player's displayed game mode

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that the built-in command sends the selected game mode to the connected client without claiming server-side gameplay enforcement.

**Prerequisites:**

- VMC-ATP-003 has passed acceptance
- the player can enter commands
- the client is connected in the Play state.

**Steps:**

1. Run /gamemode 0.
2. Observe the client game mode and the command confirmation.
3. Run /gamemode 1.
4. Try an out-of-range mode value.

**Expected results:** Valid modes 0–3 produce the corresponding client game-mode event and confirmation; an out-of-range value is rejected by command parsing; the connection remains stable.

### VMC-ATP-011: Teleport a player

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that the teleport command authoritatively moves a player to supplied coordinates.

**Prerequisites:**

- VMC-ATP-003 has passed acceptance
- the player can enter commands
- a safe destination coordinate is known.

**Steps:**

1. Record the current position.
2. Run /tp with a safe destination.
3. Observe the player position and orientation.
4. Move after the teleport.

**Expected results:** The player arrives at the requested coordinates; the client acknowledges the server position; subsequent movement remains responsive and no duplicate player entity is created.

### VMC-ATP-012: Manage connected players

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that the built-in list, private-message, broadcast, and kick commands affect connected players as documented.

**Prerequisites:**

- VMC-ATP-005 has passed acceptance
- both players can use commands
- the local test setup permits the list, messaging, broadcast, and kick commands.

**Steps:**

1. Run /list and verify both players are listed.
2. Send /tell from Player A to Player B.
3. Run /broadcast and verify both clients receive it.
4. Run /kick for Player B.

**Expected results:** The online list reflects connected players; private and broadcast messages reach only their intended recipients; the kicked client disconnects and the remaining client observes the player removal.

### VMC-ATP-013: Give and clear inventory items

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that the built-in item commands update the server-authoritative player inventory.

**Prerequisites:**

- VMC-ATP-003 has passed acceptance
- the player can enter commands
- a writable player inventory is available.

**Steps:**

1. Run /give minecraft:stone 20.
2. Inspect the inventory.
3. Run /give minecraft:ender_pearl 20.
4. Run /clear.
5. Inspect the inventory again.

**Expected results:** The requested item counts appear in valid stacks; per-item stack limits are respected; /clear removes the inventory contents and the client receives the updated slots.

### VMC-ATP-014: Manipulate inventory slots

**Category:** User interface  
**ATP status:** Done

**Objective:** Confirm that the server-authoritative inventory handles the supported vanilla click operations.

**Prerequisites:**

- VMC-ATP-003 has passed acceptance
- the player has at least two item stacks
- the inventory screen is available.

**Steps:**

1. Pick up and place a stack with left and right clicks.
2. Shift-click an item.
3. Swap a hotbar item with a number key or the offhand.
4. Drag across multiple slots.
5. Double-click to collect matching items.

**Expected results:** Each operation produces the expected stack counts and slot contents; the client and server converge after every action; invalid or stale actions do not duplicate items or crash the server.

### VMC-ATP-015: Drop and collect an item

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that a thrown item becomes a visible world entity and can be collected by a nearby player after its pickup delay.

**Prerequisites:**

- VMC-ATP-005 has passed acceptance
- Player A has a disposable item
- both players are within a few blocks of a safe floor.

**Steps:**

1. Drop one item from Player A's inventory.
2. Observe the item entity and wait through the pickup delay.
3. Move Player B onto the item.
4. Inspect Player B's inventory.

**Expected results:** The item entity is spawned and visible to both players; it falls without clipping through the floor; it cannot be collected before the delay; after collection the entity disappears and the stack is added once to Player B.

### VMC-ATP-016: Spawn a supported entity

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that the summon command validates a supported entity type and creates a visible non-player entity.

**Prerequisites:**

- VMC-ATP-003 has passed acceptance
- the player can enter commands
- a supported entity identifier such as minecraft:pig is known.

**Steps:**

1. Run /summon minecraft:pig at the current position.
2. Inspect the entity in the client.
3. Try a non-summonable or malformed identifier.

**Expected results:** A valid entity appears with a stable identity and position; invalid or non-summonable identifiers are rejected with a controlled command error; the server remains healthy.

### VMC-ATP-017: Apply wandering behavior to an entity

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that the summon command can attach the implemented random-walk behavior to a non-player entity.

**Prerequisites:**

- VMC-ATP-016 has passed acceptance
- a safe, open test area is available.

**Steps:**

1. Run /summon minecraft:pig --wander.
2. Observe the entity for several seconds.
3. Compare its position and movement packets with a pig summoned without --wander.

**Expected results:** The entity receives the documented Wander behavior and changes position over time; a summoned entity without the flag does not gain that behavior; no server error is emitted.

### VMC-ATP-018: Apply gravity and block collisions

**Category:** Critical feature  
**ATP status:** Done

**Objective:** Confirm that opt-in non-player physics settles entities on blocks and prevents them from clipping through simple obstacles.

**Prerequisites:**

- VMC-ATP-016 has passed acceptance
- a test area has a flat floor, a one-block rise, and a vertical wall at least two blocks high.

**Steps:**

1. Build a test area with a flat floor, a one-block rise, and a vertical wall at least two blocks high.
2. Run /summon minecraft:pig --wander --gravity --block-checks above the floor.
3. Observe the fall and resting position.
4. Let the entity move toward the one-block rise and the vertical wall.
5. Observe the entity over several ticks.

**Expected results:** The entity falls to the block top and reports a grounded state; it may step onto and traverse a one-block rise; it does not clip through the vertical wall; physics remains stable over several ticks.

**ATP feedback:** Acceptance criteria aligned with the implemented one-block step-up behavior.

### VMC-ATP-019: Stream chunks around a moving player

**Category:** Critical feature  
**ATP status:** Done

**Objective:** Confirm that chunk loading and unloading follow the player's effective view distance while the player crosses chunk boundaries.

**Prerequisites:**

- VMC-ATP-003 has passed acceptance
- the world has at least 32 blocks of safe movement
- client and server view distances are known.

**Steps:**

1. Record the initial chunk center.
2. Move across at least two chunk boundaries.
3. Observe new terrain and chunk-center updates.
4. Return toward the origin and inspect previously loaded terrain.

**Expected results:** Chunks enter and leave the player's loaded set according to the capped view distance; new chunks arrive nearest-first; returning to an already loaded area does not create duplicate or corrupted chunks.

### VMC-ATP-020: Load chunks through the world loader hook

**Category:** Critical feature  
**ATP status:** Done

**Objective:** Confirm that the loader interface is consulted before generation and that the built-in persistence adapter loads stored chunks while missing chunks use the generator fallback.

**Prerequisites:**

- A test ChunkLoader or the voidmc-world-io persistence plugin
- a temporary world directory
- one known stored chunk
- one known missing chunk.

**Steps:**

1. Start the server with the loader or persistence plugin installed.
2. Request the stored chunk.
3. Request the missing chunk.
4. Compare the returned data with the stored value and generated fallback.

**Expected results:** A loader-provided chunk is used before generation; a missing chunk follows the documented generator fallback; the built-in RegionChunkStore round-trip is preserved. No arbitrary external database backend is claimed by this scenario.

### VMC-ATP-021: Register a custom command

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that an application or plugin can add a command without modifying the framework core.

**Prerequisites:**

- Repository source
- a small application or plugin crate
- access to CommandRegistry
- the example server builds.

**Steps:**

1. Register a command with CommandBuilder.
2. Start the example server.
3. Execute the command in chat.
4. Restart without the registration and repeat the command.

**Expected results:** The registered command appears in discovery and executes its handler; removing the registration removes the command; core server startup and unrelated commands remain functional.

### VMC-ATP-022: Register a custom item behavior

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that an application can register an ItemBehavior for one item while unrelated items retain their default interaction behavior.

**Prerequisites:**

- A behavior implementing the documented ItemBehavior trait
- the example WandBehavior or an equivalent test behavior
- a connected Creative-mode player.

**Steps:**

1. Register the behavior for minecraft:stick.
2. Hold the item and use it on a valid block.
3. Use a normal block item on another block.
4. Inspect the resulting world and chat response.

**Expected results:** The registered behavior handles the targeted interaction and applies its documented world change; unrelated item placement keeps the default behavior; the server remains stable.

### VMC-ATP-023: Extend the server through the Bevy plugin hook

**Category:** Secondary function  
**ATP status:** In progress

**Objective:** Confirm that an application can install a Bevy plugin closure that registers observers, systems, or shared ECS resources without modifying framework core code.

**Prerequisites:**

- Repository source
- a plugin closure or Rust application extension
- a test event or resource
- permission to rebuild the example application.

**Steps:**

1. Add a plugin that inserts a resource and registers an observer or system.
2. Build and start the example server.
3. Trigger the event or command that uses the resource.
4. Inspect the resulting output or state.

**Expected results:** The plugin is applied before gameplay; its observer or system runs in the Bevy schedule; the shared resource is available to the registered consumer; no external Java/Spigot plugin loader is implied.

### VMC-ATP-024: Configure the server through ServerConfigBuilder

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that supported runtime parameters can be supplied through ServerConfigBuilder and are reflected in observable startup or status behavior.

**Prerequisites:**

- Repository source
- a small application or temporary configuration change
- an available local TCP port
- a status client or protocol probe.

**Steps:**

1. Set documented builder options including the MOTD, max_players, tick rate, and spawn/chunk settings.
2. Build and start the server.
3. Query status and inspect startup behavior.
4. Restore the baseline configuration.

**Expected results:** The server starts with the supplied configuration; status and startup observations reflect the configured values where exposed; no performance or capacity guarantee is inferred from configuration alone.

### VMC-ATP-025: Capture TPS and profiling diagnostics

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that the example can emit TPS CSV samples and optional flame-compatible traces when diagnostics are explicitly enabled.

**Prerequisites:**

- Repository source
- a writable logs directory
- VOID_METRICS_DEBUG=1
- optional VOID_TPS_OUTPUT
- optional VOID_METRICS_MODE=flame with VOID_FLAME_OUTPUT.

**Steps:**

1. Start the server with TPS output enabled.
2. Let it run for at least 20 ticks.
3. Stop it cleanly.
4. When flame mode is enabled, inspect the folded trace file.

**Expected results:** Enabled TPS output contains the documented header and timestamped samples; enabled flame mode produces a non-empty folded trace; an unavailable optional output path is reported by a warning without an invented success claim.

### VMC-ATP-026: Stop the server gracefully

**Category:** Critical feature  
**ATP status:** Done

**Objective:** Confirm that /stop and the supported process signals finish the current work and shut down without leaving dirty chunks unflushed.

**Prerequisites:**

- VMC-ATP-001 has passed acceptance
- persistence is enabled for the test world
- permission to stop the local server.

**Steps:**

1. Make one tracked world change.
2. Issue /stop or send the documented termination signal.
3. Wait for the process to exit.
4. Restart the same world and inspect the change.

**Expected results:** The process exits cleanly; shutdown persistence flushes dirty chunks; the world change is available after restart; no panic or partial shutdown is reported.

### VMC-ATP-027: Generate optimized worlds

**Category:** Critical feature  
**ATP status:** In progress

**Objective:** Confirm that optimized world generation produces the expected terrain and spawn area.

**Prerequisites:**

- VMC-ATP-026 has passed acceptance
- the optimized world-generation path is enabled
- a writable test-world directory
- a defined terrain fixture

**Steps:**

1. Start the server with optimized world generation enabled.
2. Generate the defined test world.
3. Connect a Minecraft client and inspect the spawn area.
4. Move across one chunk boundary and compare the terrain with the fixture.

**Expected results:** World generation completes without errors; terrain and spawn behavior match the defined fixture; generated chunks remain consistent as the player moves.

### VMC-ATP-028: Process regions and systems in parallel

**Category:** Critical feature  
**ATP status:** In progress

**Objective:** Confirm that independent regions and systems are processed concurrently without corrupting the world state.

**Prerequisites:**

- VMC-ATP-027 has passed acceptance
- multithreaded execution is available

**Steps:**

1. Start the server with multithreaded processing enabled.
2. Load two independent regions and trigger their scheduled systems.
3. Repeat the scenario while recording tick completion and world state.
4. Compare the result with the single-thread baseline.

**Expected results:** Independent regions and systems run concurrently; the resulting world state matches the baseline; no race, duplicate update, crash, or deadlock is observed.

### VMC-ATP-029: Simulate biomes and weather

**Category:** Secondary function  
**ATP status:** In progress

**Objective:** Confirm that configured biome and weather states are applied to a test area and synchronized to the client.

**Prerequisites:**

- VMC-ATP-027 has passed acceptance
- a reproducible biome and weather scenario is configured

**Steps:**

1. Start the server with the biome and weather scenario enabled.
2. Connect a Minecraft client.
3. Travel to the configured test area.
4. Observe the biome and weather state before and after the configured transition.

**Expected results:** The client displays the configured biome and weather state; the server and client remain synchronized during the transition; no unsupported world simulation is inferred.

### VMC-ATP-030: Trigger note blocks with basic redstone

**Category:** Secondary function  
**ATP status:** In progress

**Objective:** Confirm that basic redstone behavior activates note blocks.

**Prerequisites:**

- VMC-ATP-027 has passed acceptance
- a test world contains a lever or button, redstone dust, and a note block

**Steps:**

1. Start the server with the basic redstone scenario enabled.
2. Connect a Minecraft client and inspect the prepared circuit.
3. Activate the lever or button.
4. Deactivate and reactivate the circuit.
5. Repeat the check after the client rejoins the test world.

**Expected results:** A basic redstone signal reaches the note block and triggers it reliably; lever or button state is synchronized to the client after each toggle; repeaters, comparators, pistons, and complex circuits are outside this ATP's scope.

### VMC-ATP-031: Run functional and load tests

**Category:** Critical feature  
**ATP status:** In progress

**Objective:** Confirm that the server supports repeatable functional and multi-client load runs for a defined Minecraft workload.

**Prerequisites:**

- VMC-ATP-027 has passed acceptance
- a full-protocol load client
- a repeatable workload
- pass/fail thresholds
- a private staging endpoint

**Steps:**

1. Start the server at the private staging endpoint.
2. Run the functional smoke checks for the defined workload.
3. Run the full-protocol load client.
4. Record connection success, tick rate, latency, errors, and resource measurements.
5. Compare the results with the defined thresholds.

**Expected results:** The report identifies the workload, run conditions, measurements, and pass/fail decision; no capacity claim is made beyond the defined scenario and thresholds.

### VMC-ATP-032: Expose health and Prometheus metrics

**Category:** Secondary function  
**ATP status:** In progress

**Objective:** Confirm that the server exposes a health endpoint and Prometheus-compatible metrics for a running server.

**Prerequisites:**

- VMC-ATP-027 has passed acceptance
- observability endpoints are enabled
- an HTTP client
- a private test port

**Steps:**

1. Start the server with observability enabled.
2. Query the health endpoint.
3. Scrape the metrics endpoint.
4. Stop the server and query the documented unhealthy state.

**Expected results:** The health endpoint reports the documented healthy and unhealthy states; the metrics endpoint returns the agreed tick, player, and error metrics in Prometheus-compatible format.

### VMC-ATP-033: Advertise commands and complete command suggestions

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that the server advertises its command tree and returns controlled tab-completion suggestions for supported inputs.

**Prerequisites:**

- VMC-ATP-003 has passed acceptance
- a registered command
- test player and entity names

**Steps:**

1. Open chat and enter the prefix of a registered command.
2. Request suggestions at the command root and after a typed argument.
3. Check suggestions for a player name and a summonable entity name.
4. Test an unknown or incomplete prefix.

**Expected results:** The client receives the advertised command tree; suggestions contain only valid matches; player and summonable-entity suggestions appear where applicable; unknown prefixes do not crash or disconnect the server.

### VMC-ATP-034: Add custom player behavior through a Bevy plugin

**Category:** Secondary function  
**ATP status:** In progress

**Objective:** Confirm that an application can attach a Bevy observer to player lifecycle events and react to them without modifying framework core code.

**Prerequisites:**

- VMC-ATP-023 has passed acceptance
- a small application or plugin crate
- an observer that records event order and player identity

**Steps:**

1. Add the plugin to the test application and register observers for PlayerJoinEvent, PlayerReadyEvent, and PlayerQuitEvent.
2. Start the test application and connect one client.
3. Wait for initial chunks and PlayerLoaded.
4. Disconnect the client.
5. Inspect the recorded events and the plugin's reaction.

**Expected results:** PlayerJoinEvent fires after Configuration and before PlayerReady; PlayerReadyEvent fires after PlayerLoaded; PlayerQuitEvent fires when the ready player disconnects; the plugin identifies the correct player and reacts without destabilizing the server.

### VMC-ATP-035: Observe gameplay events through a Bevy plugin

**Category:** Secondary function  
**ATP status:** In progress

**Objective:** Confirm that an application can observe implemented movement, chat, block, and item interaction events through Bevy observers.

**Prerequisites:**

- VMC-ATP-023 has passed acceptance
- a small application or plugin crate
- an observer that records event payloads

**Steps:**

1. Register observers for representative PlayerMoveEvent, ChatMessageEvent, BlockPlaceEvent, BlockBreakEvent, and PlayerUseItemEvent events.
2. Move and rotate the player.
3. Send a chat message.
4. Place and break a block.
5. Use an item and inspect the recorded payloads.

**Expected results:** Each action produces the corresponding semantic event; payloads identify the correct player or entity and action data; event observation does not claim unsupported gameplay such as combat or damage.

### VMC-ATP-036: Provide a custom world generator

**Category:** Secondary function  
**ATP status:** In progress

**Objective:** Confirm that an application can replace the default terrain generator with a recognizable custom generator.

**Prerequisites:**

- VMC-ATP-023 has passed acceptance
- a custom WorldGenerator implementation
- a writable test-world directory

**Steps:**

1. Implement a custom WorldGenerator with a known block pattern and surface height.
2. Supply it through ServerConfigBuilder.world_generator.
3. Start the test application and connect a client.
4. Inspect the spawn height and generated terrain.
5. Cross a chunk boundary and inspect another generated chunk.

**Expected results:** Generated chunks use the supplied generator; the spawn position uses its surface height when no explicit spawn height is configured; the recognizable pattern remains consistent across streamed chunks.

### VMC-ATP-037: Add a custom biome

**Category:** Secondary function  
**ATP status:** In progress

**Objective:** Confirm that an application can add a custom biome with a clearly visible effect in a defined test area.

**Prerequisites:**

- VMC-ATP-023 has passed acceptance
- a custom biome definition with a client-visible effect
- a test application with the extension enabled

**Steps:**

1. Add the custom biome to the test application's world configuration.
2. Start the application and connect a client.
3. Travel to the marked test area.
4. Inspect the biome identifier and its visible effect.
5. Leave and re-enter the area to verify the result is stable.

**Expected results:** The marked area uses the custom biome; the client exposes its identifier and visible effect; the result remains stable after leaving and re-entering the area.
