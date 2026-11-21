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
| VMC-ATP-019 | Stream chunks around a moving player | Critical feature | 003 | In progress | Partial | [#139](https://github.com/VoidMinecraft/VoidMC/pull/139) | Yes (28 of 37, 17:00 UTC) | Fix + tests. Capping as min(client, server), forgetting behind the player and centre-before-chunks already worked. Fixed: (1) the join sent the whole `initial_chunk_radius` square even when the effective view distance was smaller, then forgot the extra ring; (2) nearest-first broke when the generation cap held back a nearer ungenerated chunk while farther cached chunks were sent. Seven e2e scenarios + one unit test. Judgment calls: nearest-first = squared Euclidean distance from the player's chunk (as documented); cached chunks now wait behind nearer ungenerated ones, which lowers throughput under the generation cap (vanilla sends whatever is ready). Based on main; will conflict trivially with #135/#136 on one line of the join loop. Combined behavior of #135 + #136 + #139 has not been tested together. |
| VMC-ATP-020 | Load chunks through the world loader hook | Critical feature | - | Done | Implemented | [#127](https://github.com/VoidMinecraft/VoidMC/pull/127) | Yes (8 of 37, 13:32 UTC) | Tests only. Two e2e scenarios: test ChunkLoader consulted before generator (spawn area and streaming), and WorldPersistencePlugin/RegionChunkStore round-trip across a restart. Assumption: "documented generator fallback" = active generator output; only block states compared. Adds voidmc-world-io as an e2e dev-dependency (trivial conflict risk in void-e2e/Cargo.toml and Cargo.lock). |
| VMC-ATP-021 | Register a custom command | Secondary function | - | Done | Implemented | [#133](https://github.com/VoidMinecraft/VoidMC/pull/133) | Yes (13 of 37, 14:11 UTC) | Tests only, plus a one-line docs fix (missing `StringArg` import in the CommandBuilder example). Two e2e scenarios: a command registered with `add_command` is in the tree and `/help` and runs, then the same app restarted without it gives "Unknown command"; same checks for a command registered from a plugin. Out of scope, not fixed: commands.md says the tree is sent "during the configuration phase", it is sent on entering Play. |
| VMC-ATP-022 | Register a custom item behavior | Secondary function | - | Done | Partial | [#160](https://github.com/VoidMinecraft/VoidMC/pull/160) | Yes (7 of 37, 13:23 UTC) | Fix + tests, CI green. **Based on #137 (merge #137 first):** without #137's per-hand snapshot one vanilla click runs the stick behavior twice. The ItemBehavior API is public and documented and worked for a single packet, but with a real vanilla click (three packets: use-on main hand, use-in-air, use-on off hand) the example wand did nothing useful: the empty off hand ran the default placement for the air item and placed air over the wand's glowstone (that guard now lives in #137). Bug fixed here: the break hooks used the item held when the queue drained, so a same-tick hotbar change ran another item's behavior. Eight e2e scenarios (full vanilla click, context contents, Pass vs Handled, consume, break hooks, unrelated items unchanged, replace-on-second-registration, unknown item → warning, bursts) + two unit tests. `enqueue_break`/`enqueue_start_break` gain an inventory parameter. Notes: one vanilla click on a block runs both `on_use_on_block` and `on_use` (as vanilla; documented, not suppressed); a panicking behavior takes down the game thread (untested, undocumented). Out of scope, not fixed: `place_block` and the default placement overwrite a solid neighbour (vanilla only replaces replaceable blocks). |
| VMC-ATP-023 | Extend the server through the Bevy plugin hook | Secondary function | - | Done | Implemented | [#128](https://github.com/VoidMinecraft/VoidMC/pull/128) | Yes (9 of 37, 13:39 UTC) | Tests only; ATP status "In progress" was stale. One e2e scenario: plugin closure inserts resources, a Startup and an Update system, an observer and a /probe command; client sees the command in the join tree and the observer replies. "No Java/Spigot loader implied" checked by reading the docs, not by a test. |
| VMC-ATP-024 | Configure the server through ServerConfigBuilder | Secondary function | - | Done | Partial | [#161](https://github.com/VoidMinecraft/VoidMC/pull/161) | Yes (36 of 37, 20:05 UTC) | Fix + tests, CI green. Every builder option already reached something observable and documented defaults matched the code. Fixed: a negative `view_distance` was sent to the client unchanged and left the player standing in the void (negative view/simulation distances are now replaced by 0 with a warning). Docs corrected: `max_players` is advertised only, NOT enforced (a test shows two players online with `max_players(1)`); clients are not told the tick rate; new "Limits" section stating configuration guarantees neither capacity nor performance. Seven e2e scenarios (every option set to a non-default value and observed by a client; tick rate 200 vs 10 TPS; defaults restored in the same process) + an integration test that parses the configuration reference table and compares it with the code. Judgment calls: `max_players` documented as advisory rather than adding a login refusal; an unbindable/unparsable address still panics before the first tick (documented). AFTER MERGE of #138 or #145: `void/tests/server_config.rs` will not compile until the new config field is added to its list (intended guard); rerun CI. |
| VMC-ATP-025 | Capture TPS and profiling diagnostics | Secondary function | - | Done | Partial | [#162](https://github.com/VoidMinecraft/VoidMC/pull/162) | Yes (35 of 37, 19:49 UTC) | Fix + tests, CI green. The TPS CSV and the flame trace were written correctly. Fixed: (1) an unusable flame output path aborted startup with an error instead of a warning; now a warning naming the path and reason, and the server runs on; (2) `VOID_METRICS_MODE=flame` alone also wrote an undocumented TPS CSV; TPS sampling now depends on `VOID_METRICS_DEBUG` only; (3) the TPS write-failure warning named neither path nor reason; (4) docs out of step (default paths, sampling interval, replace-not-append). Tests launch the real example binary as a child process on an ephemeral port through a new `VOID_LISTEN_ADDR` variable (default unchanged, 127.0.0.1:25565), stop it with SIGINT and check the CSV and folded trace are complete; plus unit tests and three in-process e2e scenarios. Notes: an existing CSV is replaced; the window in progress at shutdown is not written. Out of scope: on main SIGTERM loses the last trace buffer (#132 fixes graceful SIGTERM). Conflicts expected in `void-example/src/main.rs` with #130, #145, #147. |
| VMC-ATP-026 | Stop the server gracefully | Critical feature | 001 | Done | Partial | [#132](https://github.com/VoidMinecraft/VoidMC/pull/132) | Yes (12 of 37, 14:04 UTC) | Fix + tests. `/stop` and SIGINT worked; SIGTERM, documented as graceful, killed the process without flushing dirty chunks (the `ctrlc` crate only handles SIGINT without its `termination` feature). Fix enables that feature in `void/Cargo.toml`; SIGHUP becomes graceful too and is documented. Tests: process-level SIGINT/SIGTERM/SIGHUP tests in `void-world-io/tests/graceful_stop.rs` (own server process, ephemeral port, not the example binary) and an e2e `/stop` scenario. Harness gains `TestServer::wait_for_exit()`; same e2e dev-dependency line as #127 (trivial conflict). Not changed: `VoidServer::run` drops the AppExit value, so the process always exits 0. |
| VMC-ATP-027 | Generate optimized worlds | Critical feature | 026 | Done | Partial | [#135](https://github.com/VoidMinecraft/VoidMC/pull/135) | Yes (1 of 37, 12:11 UTC) | Fix + tests. Assumption: no separate "optimized" generator or switch exists, so "optimized path" = the default pipeline (spawn-area pre-generation, chunk cache, per-tick generation budget); reference path = no pre-generation and no budget. No speed-up measured or claimed. Bug fixed: when the spawn area is smaller than `initial_chunk_radius` and the view distance equals it, joining left holes around spawn until the player moved. Terrain fixture defined in the e2e scenario (column rule, heights, spawn (0,64,0), seams); optimized and reference paths proven identical. Out of scope, not fixed: public `generate_spawn_area`/`generate_chunk`/`surface_height_at` free functions ignore the configured generator. |
| VMC-ATP-028 | Process regions and systems in parallel | Critical feature | 027 | Done | Missing | [#138](https://github.com/VoidMinecraft/VoidMC/pull/138) | Yes (33 of 37, 19:06 UTC) | Implementation + tests. On main nothing ran in parallel: `bevy_ecs` was built without `multi_threaded`, so all schedules were single-threaded (the e2e workspace got the multi-threaded executor by accident through Azalea's feature unification). Added `ExecutionMode { SingleThreaded (default), MultiThreaded }` via `ServerConfigBuilder::execution_mode`, and `for_each_region` (region = 32x32 chunks of one dimension) used by entity physics and spawn settling. Tests prove concurrency with a rendezvous (not timing), byte-for-byte equality with the single-thread baseline, and exactly-once updates. Judgment calls: default stays single-threaded; only existing per-entity work is region-parallel; wander is not (uses thread RNG). KNOWN LIMIT, not fixed: Bevy reports 26 unordered conflicting system pairs in `Update`, so under MultiThreaded their order within a tick can differ from the baseline; documented in configuration.md, needs a decision on a total order. |
| VMC-ATP-029 | Simulate biomes and weather | Secondary function | 027 | Done | Implemented | [#146](https://github.com/VoidMinecraft/VoidMC/pull/146) | Yes (19 of 37, 15:09 UTC) | Tests + docs only, CI green; ATP status "In progress" was stale. Two e2e scenarios on a fixture (a snowy_plains area in a plains world, clear → thunder over a 100-tick ramp, with a mid-ramp reversal): registry carries the biomes' client-visible properties, every biome cell the client holds matches, a traveller sees the area biome and area-scoped weather on entering and clear on leaving, and each client's weather equals the server's at every tick incl. a mid-ramp joiner. Assumptions: "configured biome" = a built-in biome placed by the generator; "configured transition" = the level ramp plus a change mid-ramp; area weather = an explicit audience kept by an application system (weather has no position). Docs gain explicit "not simulated" sections. Small shared harness change: `JoinTranscript.registry_data`. |
| VMC-ATP-030 | Trigger note blocks with basic redstone | Secondary function | 027 | Done | Missing | [#147](https://github.com/VoidMinecraft/VoidMC/pull/147) | Yes (5 of 37, 13:05 UTC) | Implementation + tests, CI green. **Based on #134 (merge #134 first):** it needs that PR's `Sneaking(bool)` component for the sneak rule on lever/button clicks. Nothing existed on main. Added an opt-in `RedstonePlugin` (not in DefaultPlugins because it changes what a right-click on a lever/button does; the example enables it and adds `/circuit` to build the test circuit): levers and all buttons (20/30-tick release), flat redstone dust with power 15 decreasing by 1, note block plays once per rising edge (Sound + new Block Event packet 0x07), silent when covered, states stored so rejoiners see them. New public `BlockInteraction` trait/registry and `world::set_block_state`. Six e2e scenarios + 16 unit tests. Not supported (documented): dust up/down a block, power through solid blocks, torches, repeaters, comparators, pistons, tuning by right-click. Deviation from vanilla: registered item behaviors run before the block interaction, to keep the WorldEdit wand and navigation remote working. Out of scope, not fixed: hand placement ignores orientation/shape (a hand-placed lever is always a north wall lever, dust a dot). |
| VMC-ATP-031 | Run functional and load tests | Critical feature | 027 | In progress | Missing | [#142](https://github.com/VoidMinecraft/VoidMC/pull/142) | Yes (16 of 37, 14:36 UTC) | **Removed from the ATP on 2026-11-21:** it needed a private staging server and load hardware that are not defined, and its standard@1 thresholds depend on the host. The load tool and its e2e scenarios stay in the code. Original notes: Implementation + tests. On main only a TCP-connect stress POC existed (no login). Added a load runner in `void-e2e` (`voidmc-load` binary) reusing the full-protocol e2e bot: 7 smoke checks, then a load phase; versioned workloads `smoke@1` (4 clients, CI-sized) and `standard@1` (20 clients, 30 s, opt-in) with thresholds; in-process target by default or an operator-supplied endpoint (refuses to run without one, no default address); JSON + Markdown report with workload, run conditions, measurements, per-threshold verdicts, decision and a scope statement; exit code 0/1/2. Eight e2e scenarios incl. real induced failures. Judgment calls: latency = chat/command round trips as seen by clients; tick rate from world-time updates (the in-process run spawns a world clock; not applicable for an endpoint that sends none); thresholds chosen by the subagent, not from the ATP. Removed an unbacked "thousands of concurrent connections" claim from getting-started.md. Out of scope, not fixed: nothing refuses a login when the server is full (`max_players` is only advertised). |
| VMC-ATP-032 | Expose health and Prometheus metrics | Secondary function | 027 | Done | Missing | [#145](https://github.com/VoidMinecraft/VoidMC/pull/145) | Yes (34 of 37, 19:30 UTC) | Implementation + tests, CI green. Nothing existed on main. Added to the core crate, off by default: `ServerConfigBuilder::observability(ObservabilityConfig::localhost(port))`; example reads `VOID_OBSERVABILITY_ADDR`. `GET /health`: `starting`/`stalled`/`stopping` → 503, `ok` → 200; after stop the port refuses connections. `GET /metrics`: Prometheus text 0.0.4 with tick (count, duration histogram, target and measured TPS), player (online, max, joined, connections) and error (connection errors by kind, kicks, unrecognized packets, packet errors) metrics. HTTP runs on its own thread, no new dependencies. Seven e2e scenarios with a strict parser. Judgment calls: the "agreed" metric set and the health semantics are the subagent's proposal; lives in core rather than an opt-in crate because the counters and tick boundaries are internal. Out of scope, not fixed: `/kick` sends Disconnect but never closes the connection server-side. Conflicts expected with #132/#138 in app.rs and config.rs. |
| VMC-ATP-033 | Advertise commands and complete command suggestions | Secondary function | 003 | Done | Implemented | [#163](https://github.com/VoidMinecraft/VoidMC/pull/163) | Yes (27 of 37, 16:44 UTC) | Tests + docs only, CI green. Five e2e scenarios: the tree a client receives is well formed and complete (14 default commands + 3 aliases, right parsers, ranges, executable flags, `/summon` flags); command-name completion works from the tree; server-side completion for `ask_server` arguments returns only valid matches with UTF-16 ranges and follows players joining and leaving; hostile and flooding requests (32,500-character lines, control characters, 500 requests in a burst) get the documented reply or none, with no error and no kick; a not-yet-ready player is answered. Who completes what: a vanilla client completes names, flags, `/tell`/`/kick` players and `/summon` entities by itself; the server only answers for custom `ask_server` arguments (no default command has one). Docs fixed: the tree is sent on entering Play, not "during configuration". Judgment calls: no reply to an unknown or bare command name counts as controlled; suggestions are not sorted (documented). Out of scope, not fixed: the server never sends the op-level status to clients, so a real client may not offer selectors like `@s`; a request with more than 32 KiB of multibyte text is logged at ERROR. |
| VMC-ATP-034 | Add custom player behavior through a Bevy plugin | Secondary function | 023 | Done | Partial | [#151](https://github.com/VoidMinecraft/VoidMC/pull/151) | Yes (29 of 37, 17:27 UTC) | Fix + tests, CI green. The normal Join → Ready → Quit path worked with public API only. Five defects fixed: (1) a repeated PlayerLoaded fired PlayerReadyEvent twice; (2) a player who joined and left before PlayerLoaded got a join and never a quit; (3) no quit event at server shutdown; (4) `/kick` never closed the connection, so a client ignoring Disconnect stayed in the game; (5) an observer despawning a player entity panicked the server on that client's next packet. New public `Players::kick(entity, reason)`; kicks now flush the reason then close (2 s bound). Five e2e scenarios with a recording plugin + six unit tests. BEHAVIOUR CHANGES to review: PlayerQuitEvent now pairs with PlayerJoinEvent (fires for joined-but-never-ready players; check `Has<PlayerReady>` for the old meaning; the demo's quit observer was adapted); quit now fires for every player at shutdown; despawning a client entity closes its connection but fires no quit (documented as unsupported, use `Players::kick`). Out of scope, not fixed: keep-alive timeout only covers ready players. Conflicts expected with #145 (network.rs, server.rs). |
| VMC-ATP-035 | Observe gameplay events through a Bevy plugin | Secondary function | 023 | Done | Partial | [#150](https://github.com/VoidMinecraft/VoidMC/pull/150) | Yes (22 of 37, 15:42 UTC) | Fix + tests, CI green. Move, rotate, chat, block place/break and item-use events were correct. Fixed: `PlayerCommandAction` used pre-1.21.2 numbering, so a real client's sprint fired `PlayerSneakEvent{false}` and sprint events never fired; now protocol-775 numbering, sneak events come from Player Input and sprint events from Player Command (`StartSneaking`/`StopSneaking` variants removed from voidmc-protocol: breaking, unused in the repo). Four e2e scenarios with a recording plugin for 22 event types that assert on the whole event log, with two players. events.md now states firing rules and that events are client input, not gameplay (no combat/damage). Assumption: a packet repeating the current position still fires a move event (existing behaviour, now documented). Out of scope, NOT fixed: `Interact` is decoded with the pre-26.1 layout and the separate `Attack` packet is not decoded, so real attacks produce no event and an off-hand entity click reads as an attack (sources disagree on the wire layout); placement does not check the target is replaceable. |
| VMC-ATP-036 | Provide a custom world generator | Secondary function | 023 | Done | Partial | [#148](https://github.com/VoidMinecraft/VoidMC/pull/148) | Yes (20 of 37, 15:20 UTC) | Fix + tests, CI green. The configured generator was already used everywhere (spawn area, streaming, spawn height, loader fallback). Bug fixed: a section with more than 256 distinct block states was encoded with a palette list at 9+ bits, which a client cannot decode (Azalea panicked); such sections are now sent as raw global ids. Three e2e scenarios with a recording custom generator (world-coordinate pattern, column-varying surface, non-origin spawn (37.5, -21.5) → surface of column (37, -22); explicit spawn height wins; the documented example runs). `generate_spawn_area` (always default generator, unused by the server) is deprecated; `generate_chunk`/`surface_height_at` documented as default-only. Docs example fixed. Out of scope, not fixed: `ChunkSection::set_block_state` (block edits) still caps its palette at 8 bits, so a 257th distinct state in a section silently becomes another block. May conflict with #149 in void-protocol chunk.rs. |
| VMC-ATP-037 | Add a custom biome | Secondary function | 023 | Done | Partial | [#149](https://github.com/VoidMinecraft/VoidMC/pull/149) | Yes (21 of 37, 15:31 UTC) | Fix + tests, CI green. A custom biome could already be registered, placed (generator or `set_biome`) and synced with public API only. Fixed: (1) registering after clients had the registries was only a debug assertion, so in release builds it silently desynchronized clients; now `BiomeError::RegistriesAlreadySent`; (2) malformed identifiers reached the registry packet; now `BiomeError::InvalidName`; (3) a generator-placed custom biome with id ≥ 128 in a section with more than 8 biomes was masked to biome 0. Example `/biome` gains a radius so the effect is visible (no automated test for the example change). Three e2e scenarios incl. a registry that crosses the 7→8-bit palette boundary, decoded `ChunksBiomes`, leave/re-enter, rejoin and late joiner. Assumption: required biome fields inferred from vanilla data (no 26.1.2 jar). Harness: adds `registry_data` (same lines as #146) and `tags` to JoinTranscript. Out of scope, not fixed: `RegistryDataStore::add_entry`/`remove_entry` and the public `registries` field can still change registries after login. |

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

## Definitions

The terms below have one meaning in the whole plan. In the workbook they are on the Info & Access sheet.

- <a id="def-commit-under-test"></a>**commit under test**: The commit checked out for the test run. Record its full hash (git rev-parse HEAD) in the test report.
- <a id="def-server"></a>**server**: The example server voidmc-example. Start it at the repository root with the command that the objective gives.
- <a id="def-server-log"></a>**server log**: The newest file logs/void-*.log in the directory the server was started from (the repository root). Each server start creates a new file.
- <a id="def-tester-terminal"></a>**tester terminal**: A second terminal, open at the repository root, for the tools.
- <a id="def-client"></a>**client**: Minecraft Java Edition 26.1.2, started from Prism Launcher.
- <a id="def-player-a"></a>**Player A**: The Prism Launcher offline account ATPPlayerA. To create it, select Accounts > Manage Accounts > Add Offline and type ATPPlayerA. To start a client with it, run prismlauncher --launch <instance name> --profile ATPPlayerA.
- <a id="def-player-b"></a>**Player B**: The offline account ATPPlayerB, created like Player A. It gets a UUID different from Player A. It needs its own client instance: right-click the first instance and select Copy.
- <a id="def-player-c"></a>**Player C**: The offline account ATPPlayerC, created like Player A, with a third client instance.
- <a id="def-join"></a>**join**: In the client, select Multiplayer > Direct Connection, type the address, then click Join Server.
- <a id="def-chat"></a>**chat**: The chat box, opened with T. Type a command with its leading /, then press Enter.
- <a id="def-f3-screen"></a>**F3 screen**: The debug overlay, shown and hidden with F3. The plan reads its XYZ, Chunk, Facing, Targeted Block and biome lines. To show the biome line, press F3+F6, set biome to In Overlay, then click Done.
- <a id="def-operator-access"></a>**operator access**: Start the server with VOID_EXAMPLE_OPERATORS=1. Every player is then an operator. Operators are needed for /broadcast, /clear, /gamemode, /give, /kick, /say, /stop, /summon and /tp.
- <a id="def-fresh-world"></a>**fresh world**: No world/ directory at the repository root. If one exists, rename it, for example to world.before-atp.
- <a id="def-tools"></a>**tools**: The files under tools/atp/ in the repository (see tools/atp/README.md). The scripts need Python 3.10 or later and the standard library only.

## Details

### VMC-ATP-001: Start the example server

**Category:** Critical feature  
**ATP status:** Done

**Objective:** Confirm that the repository source builds and starts the supported example server.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [commit under test](#def-commit-under-test), [server log](#def-server-log), [Player A](#def-player-a), [join](#def-join).
- The [commit under test](#def-commit-under-test) is checked out. Record its full hash (git rev-parse HEAD) in the test report.
- Rust toolchain 1.91.1 or later (rustc --version).
- TCP port 25565 is free (ss -ltn shows no listener on 25565).
- The terms used in this plan ([server log](#def-server-log), [join](#def-join), [Player A](#def-player-a) and others) are defined in Info & Access > Definitions.

**Steps:**

1. Open a terminal at the repository root.
2. Run cargo run -p voidmc-example.
3. Wait until the terminal shows no new lines for 10 seconds.
4. Open the [server log](#def-server-log).
5. Wait 60 seconds.
6. Read the server log again.

**Expected results:**

1. Step 2: the build ends without a line that starts with "error".
2. Step 4: the [server log](#def-server-log) contains a line with "Server listening" and "127.0.0.1:25565".
3. Step 4: the server log contains "Generated 81 spawn area chunks".
4. Step 6: the server process is still running.
5. The server log contains no line with "ERROR" or "panicked".

### VMC-ATP-002: Display the server status

**Category:** Critical feature  
**ATP status:** Done

**Objective:** Confirm that a compatible status client receives the server discovery information.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [tester terminal](#def-tester-terminal), [client](#def-client), [Player A](#def-player-a), [join](#def-join), [tools](#def-tools).
- VMC-ATP-001 has passed acceptance.
- The [Player A](#def-player-a) account exists.
- Python 3.10 or later (python3 --version).

**Steps:**

1. At the repository root, run cargo run -p voidmc-example.
2. Wait until the [server log](#def-server-log) contains "Generated 81 spawn area chunks".
3. In the [client](#def-client), select Multiplayer > Add Server.
4. Type 127.0.0.1:25565 as the server address, then click Done.
5. Read the server entry in the server list.
6. In the [tester terminal](#def-tester-terminal), run python3 tools/atp/status-ping.py --server 127.0.0.1:25565.
7. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a).
8. In the tester terminal, run python3 tools/atp/status-ping.py --server 127.0.0.1:25565 again.

**Expected results:**

1. Step 5: the entry shows the MOTD "Welcome to Void Server!" and "0/100". It shows no "Incompatible version" text.
2. Step 6: the tool prints "version name: 26.1.2", "protocol: 775" and "motd: Welcome to Void Server!".
3. Step 6: the tool prints "max players: 100", "online players: 0" and "ping echo: yes".
4. Step 8: the tool prints "online players: 1".
5. Steps 6 and 8: each command ends in less than 5 seconds.

### VMC-ATP-003: Complete the implemented login and configuration handshake

**Category:** Critical feature  
**ATP status:** Done

**Objective:** Confirm that a compatible client completes the implemented Handshake, Login, Configuration, and Play transitions and becomes ready, without online authentication or encryption. Compression is optional.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [client](#def-client), [Player A](#def-player-a), [join](#def-join), [chat](#def-chat), [F3 screen](#def-f3-screen).
- VMC-ATP-001 has passed acceptance.
- The [Player A](#def-player-a) account exists.
- TCP port 25565 is free.
- No other [client](#def-client) is connected to the server.

**Steps:**

1. At the repository root, run VOID_PACKET_DEBUG=1 cargo run -p voidmc-example.
2. Wait until the [server log](#def-server-log) contains "Generated 81 spawn area chunks".
3. Start the [client](#def-client) with [Player A](#def-player-a).
4. [Join](#def-join) 127.0.0.1:25565.
5. Wait 10 seconds after the world appears.
6. Hold Tab and read the player list.
7. Open [chat](#def-chat) and type / (one slash). Do not press Enter.
8. Read the command suggestions above the chat box, then press Esc.
9. Press [F3](#def-f3-screen) and read the XYZ line.
10. Open the server log.
11. Find the line that contains "Received LoginStart" and "ATPPlayerA".
12. Record the client number in that line, for example [Client 1].
13. Read the lines with that client number that contain "Received".

**Expected results:**

1. Step 4: the [client](#def-client) joins without an authentication, encryption or "Failed to log in" error. An offline account can only [join](#def-join) a server that requires neither.
2. Step 5: the client shows terrain around the player and no disconnect screen. The client cannot show the world without the registry data and tags.
3. Step 6: the player list shows ATPPlayerA.
4. Step 8: the suggestions contain help and ping. This shows that the client received the command tree.
5. Step 9: the XYZ line shows 0.0, 64.0 and 0.0, rounded to one decimal.
6. Step 13: these Received lines occur in this order: Handshake, LoginStart, LoginAcknowledged, ClientInformation, KnownPacks, FinishConfigurationAcknowledged, PlayerLoaded. Other lines can occur between them.
7. Step 13: the Handshake line contains "protocol_version: 775" and "next_state: Login".
8. Step 13: a "Received ChunkBatchReceived" line and a "Received ConfirmTeleportation" line with "teleport_id: 0" come before PlayerLoaded.
9. After the PlayerLoaded line, the [server log](#def-server-log) contains a line with "Player connected" and "ATPPlayerA".
10. The server log contains no line with "ERROR" or "panicked".
Note: the example negotiates compression at threshold 256. These results do not depend on compression.

**ATP feedback:** Verified on 2026-11-20 at 0220a6b: Pass. The packet trace is kept in the verification evidence (uncompressed-frame-summary.txt) and as an e2e scenario (PR #131).

### VMC-ATP-004: Load the initial world

**Category:** Critical feature  
**ATP status:** Done

**Objective:** Confirm that the connected player receives and renders the initial chunk area while moving locally.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [Player A](#def-player-a), [join](#def-join), [F3 screen](#def-f3-screen).
- VMC-ATP-003 has passed acceptance.
- The [Player A](#def-player-a) account exists.
- The server is started with cargo run -p voidmc-example. Players [join](#def-join) in Creative mode, the server default.

**Steps:**

1. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a).
2. Wait until terrain is visible around the player.
3. Press [F3](#def-f3-screen). Take a screenshot with F2.
4. Move the mouse to turn 360 degrees in 4 steps of 90 degrees. Read the F3 Facing line at each step.
5. Turn until the F3 Facing line shows "south (Towards positive Z)".
6. Double-press Space to fly.
7. Fly forward until the F3 XYZ line shows Z of 128 or more. Take a screenshot each time Z passes a multiple of 32.
8. Turn until the F3 Facing line shows "north (Towards negative Z)".
9. Fly forward until Z is 0 or less.
10. Fly to X 0, Z 0 and land. Take a screenshot facing the same way as in step 3.
11. Compare the screenshots of steps 3 and 10.
12. Disconnect, then read the [server log](#def-server-log).

**Expected results:**

1. Step 2: terrain is visible in less than 10 seconds.
2. Step 4: terrain is visible in all four directions. No area of empty sky remains after you stop for 5 seconds.
3. Step 7: new terrain appears ahead of the player in each screenshot.
4. Steps 7 and 9: the player moves when you press the keys and stops when you release them. The player does not jump back to an earlier position.
5. Step 11: the terrain around spawn is the same in both screenshots. There are no holes or missing chunks.
6. Step 12: the [server log](#def-server-log) has no line with "Disconnect" or "WARN" for ATPPlayerA before step 12.
7. The server log contains no line with "ERROR" or "panicked".

### VMC-ATP-005: Synchronize two players

**Category:** Critical feature  
**ATP status:** Done

**Objective:** Confirm that two connected players can see each other's presence and movement in the same world.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [client](#def-client), [Player A](#def-player-a), [Player B](#def-player-b), [join](#def-join), [F3 screen](#def-f3-screen).
- VMC-ATP-003 has passed acceptance.
- The [Player A](#def-player-a) and [Player B](#def-player-b) accounts exist, each with its own [client](#def-client) instance.
- The server is started with cargo run -p voidmc-example.

**Steps:**

1. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a).
2. Join 127.0.0.1:25565 with [Player B](#def-player-b).
3. In the [server log](#def-server-log), find the two lines with "Player connected". Record both player_uuid values.
4. With Player A, walk 4 blocks towards positive X. Use the [F3](#def-f3-screen) XYZ line to check X is 4.
5. Turn both players to face each other.
6. With Player A, walk 3 blocks to the side, then back.
7. With Player A, jump 3 times.
8. With Player A, hold Shift for 3 seconds.
9. With Player A, turn 90 degrees right, then 180 degrees, then back. Then look fully up and fully down.
10. Repeat steps 6 to 9 with Player B.
11. Disconnect Player B.

**Expected results:**

1. Step 3: the two player_uuid values are different.
2. Step 5: each [client](#def-client) shows exactly one model of the other player, with the name above it.
3. Steps 6 to 8 and 10: the other client shows the walk, the jumps and the crouch at the same time, within 1 second.
4. Step 9 and 10: the other client shows the body turn to the side and to the back, and the head look up and down.
5. Steps 5 to 10: no client shows a second model or a model that stops moving while the player moves.
6. Step 11: the model of [Player B](#def-player-b) disappears from [Player A](#def-player-a)'s client in less than 5 seconds.
7. The [server log](#def-server-log) contains no line with "ERROR" or "panicked".

### VMC-ATP-006: Modify shared blocks

**Category:** Critical feature  
**ATP status:** Done

**Objective:** Confirm that block breaking and placement update the authoritative world and are broadcast to nearby players.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [client](#def-client), [Player A](#def-player-a), [Player B](#def-player-b), [F3 screen](#def-f3-screen), [operator access](#def-operator-access).
- VMC-ATP-005 has passed acceptance.
- The [Player A](#def-player-a) and [Player B](#def-player-b) accounts exist. Both players are in Creative mode, the server default.
- [Operator access](#def-operator-access): start the server with VOID_EXAMPLE_OPERATORS=1 cargo run -p voidmc-example.
- Player A and Player B stand at X 0, Z 0 and X -2, Z 0. Both face the block at X 0, Y 63, Z 3.
- The top block at X 0, Z 3 is at Y 63 in the default terrain.

**Steps:**

Use the [F3](#def-f3-screen) "Targeted Block" line to find each block position.
1. With [Player A](#def-player-a), break the block at X 0, Y 63, Z 3.
2. Look at the position from both [clients](#def-client).
3. With [Player B](#def-player-b), select glowstone in the hotbar (slot 5).
4. With Player B, place the glowstone at X 0, Y 63, Z 3.
5. Look at the position from both clients.
6. With Player B, run /tp 0 80 400.
7. With Player A, place a glass block on top of the block at X 0, Y 63, Z 3.
8. With Player B, run /tp -2 64 0.
9. Look at X 0, Y 64, Z 3 from both clients.
10. With Player A, run /tp 0 80 400.
11. With Player B, break the glass block at X 0, Y 64, Z 3.
12. With Player A, run /tp 0 64 0.
13. Look at X 0, Y 64, Z 3 from both clients.
14. With Player A, right-click into the sky with an empty hand.
15. With Player A, try to place stone in the block where Player A stands.
16. With Player A, click 25 times fast on the glowstone, alternating left and right click.
17. With Player A, run /tp 0 318 0, double-press Space to fly, then try to place stone above the player.

**Expected results:**

1. Step 2: both [clients](#def-client) show the block at X 0, Y 63, Z 3 is gone.
2. Step 5: both clients show glowstone at X 0, Y 63, Z 3.
3. Step 9: both clients show glass at X 0, Y 64, Z 3.
4. Step 13: both clients show air at X 0, Y 64, Z 3.
5. Steps 14 to 17: both clients stay connected. After each step, both clients show the same blocks around the target.
6. Step 15: no block appears in the player's own position.
7. Step 17: no block appears above Y 319.
8. The [server log](#def-server-log) contains no line with "ERROR", "WARN" or "panicked".

### VMC-ATP-007: Exchange chat messages

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that chat entered by one player is delivered to all connected players.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [client](#def-client), [Player A](#def-player-a), [Player B](#def-player-b), [chat](#def-chat).
- VMC-ATP-005 has passed acceptance.
- The [Player A](#def-player-a) and [Player B](#def-player-b) accounts exist. Both players are connected to the server.

**Steps:**

1. With [Player A](#def-player-a), open [chat](#def-chat), type ATP alpha 4711, then press Enter.
2. Read the chat on both [clients](#def-client).
3. With [Player B](#def-player-b), open chat, type ATP beta 0815, then press Enter.
4. Read the chat on both clients.
5. Wait 10 seconds.

**Expected results:**

1. Step 2: both [clients](#def-client) show the line "<ATPPlayerA> ATP alpha 4711" exactly once.
2. Step 4: both clients show the line "<ATPPlayerB> ATP beta 0815" exactly once.
3. Step 4: on both clients the alpha line comes before the beta line.
4. Step 5: both clients are still connected and the server is running.

### VMC-ATP-008: Discover available commands

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that a connected player can discover representative commands and receive a controlled response for invalid input.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [client](#def-client), [Player A](#def-player-a), [join](#def-join), [chat](#def-chat), [operator access](#def-operator-access).
- VMC-ATP-003 has passed acceptance.
- The [Player A](#def-player-a) account exists.
- [Operator access](#def-operator-access): start the server with VOID_EXAMPLE_OPERATORS=1 cargo run -p voidmc-example.

**Steps:**

1. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a).
2. In [chat](#def-chat), run /help.
3. Read the chat. Scroll up with the mouse wheel if the list is longer than the chat box.
4. In chat, run /ping.
5. In chat, run /plugins.
6. In chat, run /atpunknown.
7. Wait 10 seconds.

**Expected results:**

1. Step 3: the reply starts with "--- Available Commands ---". It contains "/help", "/ping", "/plugins", "/gamemode", "/give", "/tp" and "/hello - Greet the player".
2. Step 4: the reply is "Pong!".
3. Step 5: the reply is "No plugins loaded.". This is the explicit empty-list response.
4. Step 6: the reply is "Unknown command: /atpunknown" in red.
5. Step 7: the [client](#def-client) is still connected and the server is running.

### VMC-ATP-009: Preserve a world change after restart

**Category:** Critical feature  
**ATP status:** Done

**Objective:** Confirm that an accepted world modification survives a controlled server restart.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [Player A](#def-player-a), [join](#def-join), [chat](#def-chat), [F3 screen](#def-f3-screen), [operator access](#def-operator-access), [fresh world](#def-fresh-world).
- VMC-ATP-003 and VMC-ATP-006 have passed acceptance.
- The [Player A](#def-player-a) account exists.
- A [fresh world](#def-fresh-world).
- [Operator access](#def-operator-access): start the server with VOID_EXAMPLE_OPERATORS=1 cargo run -p voidmc-example.

**Steps:**

1. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a).
2. Select glowstone in the hotbar (slot 5).
3. Place the glowstone on top of the block at X 0, Y 63, Z 3. Use the [F3](#def-f3-screen) Targeted Block line.
4. Point at the glowstone. Check that the F3 Targeted Block line shows 0, 64, 3 and minecraft:glowstone.
5. Wait 15 seconds.
6. In [chat](#def-chat), run /stop.
7. Wait until the server process ends.
8. At the repository root, run [VOID_EXAMPLE_OPERATORS](#def-operator-access)=1 cargo run -p voidmc-example.
9. Wait until the [server log](#def-server-log) contains "Generated 81 spawn area chunks".
10. Join 127.0.0.1:25565 with Player A.
11. Point at X 0, Y 64, Z 3. Read the F3 Targeted Block line.

**Expected results:**

1. Step 5: the [server log](#def-server-log) contains a "World saved" line after step 3.
2. Step 6: the [chat](#def-chat) shows "Stopping the server...". The server log contains "world persistence: flushed on shutdown".
3. Step 7: the process ends in less than 10 seconds.
4. Step 9: the server log of the restart contains no line with "ERROR", "corrupt" or "panicked".
5. Step 10: [Player A](#def-player-a) joins without an error.
6. Step 11: the [F3](#def-f3-screen) Targeted Block line shows 0, 64, 3 and minecraft:glowstone.

### VMC-ATP-010: Change a player's displayed game mode

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that the built-in command sends the selected game mode to the connected client without claiming server-side gameplay enforcement.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [client](#def-client), [Player A](#def-player-a), [join](#def-join), [chat](#def-chat), [operator access](#def-operator-access).
- VMC-ATP-003 has passed acceptance.
- The [Player A](#def-player-a) account exists.
- [Operator access](#def-operator-access): start the server with VOID_EXAMPLE_OPERATORS=1 cargo run -p voidmc-example.

**Steps:**

1. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a).
2. In [chat](#def-chat), run /gamemode 0.
3. Read the chat and look at the screen.
4. In chat, run /gamemode 1.
5. Read the chat and look at the screen.
6. In chat, run /gamemode 2.
7. Read the chat and look at the screen.
8. In chat, run /gamemode 3.
9. Read the chat and look at the screen.
10. In chat, run /gamemode 4.
11. Read the chat.
12. In chat, run /gamemode 1.

**Expected results:**

1. Step 3: the reply is "Game mode set to survival (0)". The hotbar shows hearts and a hunger bar.
2. Step 5: the reply is "Game mode set to creative (1)". The hearts and hunger bar are gone.
3. Step 7: the reply is "Game mode set to adventure (2)". The hearts and hunger bar are shown.
4. Step 9: the reply is "Game mode set to spectator (3)". The hotbar is gone and the player can fly through blocks.
5. Step 11: the reply is a red error and the game mode does not change.
6. Steps 2 to 12: the [client](#def-client) stays connected.
Note: the server sends the mode to the client. It does not claim server-side enforcement of the mode's rules.

### VMC-ATP-011: Teleport a player

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that the teleport command authoritatively moves a player to supplied coordinates.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [Player A](#def-player-a), [Player B](#def-player-b), [join](#def-join), [chat](#def-chat), [F3 screen](#def-f3-screen), [operator access](#def-operator-access).
- VMC-ATP-003 has passed acceptance.
- The [Player A](#def-player-a) and [Player B](#def-player-b) accounts exist.
- [Operator access](#def-operator-access): start the server with VOID_EXAMPLE_OPERATORS=1 cargo run -p voidmc-example.

**Steps:**

1. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a), then with [Player B](#def-player-b).
2. With Player B, run /tp 12.5 70 8.5. Turn Player B to face south.
3. With Player A, press [F3](#def-f3-screen). Record the XYZ line and the Facing line.
4. With Player A, run /tp 12.5 71 12.5.
5. With Player A, read the F3 XYZ line and Facing line.
6. With Player B, look at the place where Player A stood before step 4, then at X 12.5, Z 12.5.
7. With Player A, walk forward for 3 seconds.
8. With Player A, read the F3 XYZ line.
9. With Player A, run /tp 100 80 100, then /tp 0 64 0.

**Expected results:**

1. Step 4: the [chat](#def-chat) shows "Teleported to 12.5, 71.0, 12.5".
2. Step 5: the XYZ line shows 12.5, 71.0 and 12.5, rounded to one decimal. The yaw and pitch on the Facing line are the same as in step 3.
3. Step 6: [Player B](#def-player-b) sees one model of [Player A](#def-player-a) at X 12.5, Z 12.5 and none at the old position.
4. Steps 7 and 8: Player A moves away from 12.5, 12.5 and does not jump back to it.
5. Step 9: each teleport moves Player A to the given position and terrain appears there.
6. The [server log](#def-server-log) contains no line with "ERROR" or "panicked".

### VMC-ATP-012: Manage connected players

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that the built-in list, private-message, broadcast, and kick commands affect connected players as documented.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [client](#def-client), [Player A](#def-player-a), [Player B](#def-player-b), [Player C](#def-player-c), [join](#def-join), [chat](#def-chat), [operator access](#def-operator-access).
- VMC-ATP-005 has passed acceptance.
- The [Player A](#def-player-a), [Player B](#def-player-b) and [Player C](#def-player-c) accounts exist, each with its own [client](#def-client) instance.
- [Operator access](#def-operator-access): start the server with VOID_EXAMPLE_OPERATORS=1 cargo run -p voidmc-example.

**Steps:**

1. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a), [Player B](#def-player-b) and [Player C](#def-player-c).
2. With Player A, run /list.
3. With Player A, run /tell ATPPlayerB private-gamma-27845.
4. Read the [chat](#def-chat) on all three [clients](#def-client).
5. With Player A, run /broadcast ATP broadcast 31337.
6. Read the chat on all three clients.
7. Move Player A next to Player B.
8. With Player A, run /kick ATPPlayerB ATP-test.
9. Look at Player B's screen and at Player A's view of Player B.

**Expected results:**

1. Step 2: the reply is "There are 3 player(s) online:" followed by the three names.
2. Step 4: [Player A](#def-player-a) shows "You whisper to ATPPlayerB: private-gamma-27845".
3. Step 4: [Player B](#def-player-b) shows "ATPPlayerA whispers to you: private-gamma-27845".
4. Step 4: [Player C](#def-player-c) shows no line with private-gamma-27845.
5. Step 6: all three [clients](#def-client) show "[Broadcast] ATP broadcast 31337" exactly once.
6. Step 8: Player A shows "Kicked ATPPlayerB (reason: ATP-test)".
7. Step 9: Player B shows a disconnect screen with "ATP-test". The model of Player B disappears from Player A's view.
8. Steps 8 and 9: Player A and Player C stay connected.

### VMC-ATP-013: Give and clear inventory items

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that the built-in item commands update the server-authoritative player inventory.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [Player A](#def-player-a), [join](#def-join), [chat](#def-chat), [operator access](#def-operator-access).
- VMC-ATP-003 has passed acceptance.
- The [Player A](#def-player-a) account exists.
- [Operator access](#def-operator-access): start the server with VOID_EXAMPLE_OPERATORS=1 cargo run -p voidmc-example.

**Steps:**

1. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a).
2. In [chat](#def-chat), run /gamemode 0, so that E opens the inventory with all slots.
3. In chat, run /clear.
4. In chat, run /give minecraft:stone 20.
5. In chat, run /give minecraft:ender_pearl 20.
6. Press E. Read the item counts in every slot.
7. Press Esc.
8. In chat, run /clear.
9. Press E. Read every slot.

**Expected results:**

1. Step 3: the reply is "Inventory cleared".
2. Step 4: the reply is "Gave 20 x minecraft:stone".
3. Step 5: the reply is "Gave 20 x minecraft:ender_pearl".
4. Step 6: one slot holds 20 stone. Two slots hold ender pearls: 16 and 4. No slot holds more than 16 ender pearls.
5. Step 8: the reply is "Inventory cleared".
6. Step 9: every inventory slot, hotbar slot and the offhand slot is empty.

### VMC-ATP-014: Manipulate inventory slots

**Category:** User interface  
**ATP status:** Done

**Objective:** Confirm that the server-authoritative inventory handles the supported vanilla click operations.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [tester terminal](#def-tester-terminal), [client](#def-client), [Player A](#def-player-a), [join](#def-join), [chat](#def-chat), [operator access](#def-operator-access), [tools](#def-tools).
- VMC-ATP-003 has passed acceptance.
- The [Player A](#def-player-a) account exists.
- Python 3.10 or later. The tools use only the standard library.
- The working tree has no changes (git status shows "nothing to commit").

**Steps:**

Slot numbers in the [server log](#def-server-log): 9 to 35 are the main inventory, by row from top-left.
Slots 36 to 44 are the hotbar, from left. Slot 45 is the offhand.
After each of steps 6 to 19, read the last "ATP inventory" line in the server log.
Part 1: vanilla [client](#def-client)
1. At the repository root, run git apply tools/atp/patches/inventory-trace.patch.
2. Run [VOID_EXAMPLE_OPERATORS](#def-operator-access)=1 cargo run -p voidmc-example.
3. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a).
4. In [chat](#def-chat), run /gamemode 0.
5. In chat, run /clear.
6. In chat, run /give minecraft:stone 20.
7. In chat, run /give minecraft:dirt 16.
8. Press E to open the inventory.
9. Hold Shift and left-click the stone in hotbar slot 1.
10. Move the pointer over the stone and press 2.
11. Left-click the stone to pick it up.
12. Hold the left button and drag across two empty main-inventory slots. Release the button.
13. Double-click one of the two stone stacks.
14. Left-click an empty main-inventory slot.
15. Right-click the stone stack from step 14.
16. Right-click an empty main-inventory slot.
17. Left-click a different empty main-inventory slot.
18. Press Esc to close the inventory.
19. Press E to open the inventory again.
Part 2: invalid and stale actions
20. Press Esc, then select Disconnect.
21. In the [tester terminal](#def-tester-terminal), run python3 tools/atp/inventory-clicks.py --server 127.0.0.1:25565 --server-log <server log path>.
22. Wait until the tool prints "done".
23. Join 127.0.0.1:25565 again with Player A.
24. Stop the server with Ctrl+C.
25. Run git apply -R tools/atp/patches/inventory-trace.patch to remove the patch.

**Expected results:**

1. Step 7: hotbar slot 1 holds 20 stone. Hotbar slot 2 holds 16 dirt.
2. Step 9: the 20 stone move to the top-left main-inventory slot (slot 9). Hotbar slot 1 is empty.
3. Step 10: the 20 stone are in hotbar slot 2 (slot 37). The 16 dirt are in slot 9.
4. Step 11: the pointer holds 20 stone. Slot 37 is empty.
5. Step 12: each of the two slots holds 10 stone. The pointer is empty.
6. Step 13: the pointer holds 20 stone. Both slots are empty.
7. Step 14: the slot holds 20 stone. The pointer is empty.
8. Step 15: the pointer holds 10 stone. The slot keeps 10 stone.
9. Step 16: the slot receives 1 stone. The pointer holds 9 stone.
10. Step 17: the slot receives 9 stone. The pointer is empty. The three stacks hold 10, 1 and 9 stone.
11. Step 19: the inventory shows the same slots and counts as before step 18.
12. Steps 6 to 19: after each step, the slots and counts in the last [server log](#def-server-log) line are the same as on the screen. The cursor_count value is the stack on the pointer.
13. Step 22: for each case except "03c throw", the tool prints total(before) equal to total(after). For "03c throw" the total goes down: a throw drops items, so it does not duplicate them.
14. Step 22: for every case the tool prints conn_alive_and_server_responds=True and case=PASS.
15. Step 22: for cases 09a to 09d the tool prints "offender disconnected=True" and "server accepts a new login=True". The server log contains "Client sent a malformed packet".
16. Step 22: the tool prints "failed cases: 0".
17. Step 23: [Player A](#def-player-a) joins without an error.
18. The server log contains no line with "ERROR" or "panicked".

### VMC-ATP-015: Drop and collect an item

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that a thrown item becomes a visible world entity and can be collected by a nearby player after its pickup delay.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [client](#def-client), [Player A](#def-player-a), [Player B](#def-player-b), [join](#def-join), [F3 screen](#def-f3-screen), [operator access](#def-operator-access), [fresh world](#def-fresh-world), [tools](#def-tools).
- VMC-ATP-005 has passed acceptance.
- The [Player A](#def-player-a) and [Player B](#def-player-b) accounts exist.
- A [fresh world](#def-fresh-world).
- A stopwatch.
- The working tree has no changes.

**Steps:**

1. At the repository root, run git apply tools/atp/patches/physics-arena.patch.
2. Run [VOID_EXAMPLE_OPERATORS](#def-operator-access)=1 cargo run -p voidmc-example.
3. Wait until the [server log](#def-server-log) contains "ATP arena ready".
4. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a), then with [Player B](#def-player-b).
5. With both players, run /gamemode 0.
6. With Player B, run /clear.
7. With Player A, run /clear, then /give minecraft:dirt 16.
8. With Player A, run /tp 20.5 111 20.5. Turn until the [F3](#def-f3-screen) Facing line shows "east (Towards positive X)" and the pitch is 0.
9. With Player B, run /tp 22.5 111 20.5. Turn Player B to face Player A.
10. With Player A, select the dirt and press Q once. Start the stopwatch at the same time.
11. Watch the item and Player B's hotbar.
12. When the item disappears, stop the stopwatch.
13. With Player B, press E and read every slot.
14. Stop the server with Ctrl+C, then run git apply -R tools/atp/patches/physics-arena.patch.
15. Rename the world/ directory, for example to world.after-atp-015.

**Expected results:**

1. Step 10: [Player A](#def-player-a)'s dirt count goes from 16 to 15.
2. Step 11: both [clients](#def-client) show the dirt item flying forward and landing on the stone floor at Y 111. It does not sink into the floor.
3. Step 11: [Player B](#def-player-b)'s hotbar stays empty for at least 2 seconds after the drop.
4. Step 12: the stopwatch shows between 2 and 3 seconds. The item disappears from both clients at the same time.
5. Step 13: Player B holds exactly 1 dirt.
6. The [server log](#def-server-log) contains no line with "ERROR" or "panicked".

### VMC-ATP-016: Spawn a supported entity

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that the summon command validates a supported entity type and creates a visible non-player entity.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [client](#def-client), [Player A](#def-player-a), [Player B](#def-player-b), [join](#def-join), [operator access](#def-operator-access).
- VMC-ATP-003 has passed acceptance.
- The [Player A](#def-player-a) and [Player B](#def-player-b) accounts exist.
- [Operator access](#def-operator-access): start the server with VOID_EXAMPLE_OPERATORS=1 cargo run -p voidmc-example.

**Steps:**

1. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a), then with [Player B](#def-player-b).
2. With Player A, stand at X 0, Z 0 and face south.
3. With Player A, run /summon minecraft:pig -1 64 4.
4. Watch the pig from both [clients](#def-client) for 60 seconds.
5. With Player B, disconnect, then join again. Look at X -1, Z 4.
6. With Player A, run /summon minecraft:banana.
7. With Player A, run /list.

**Expected results:**

1. Step 3: the reply is "Summoned minecraft:pig at -1.0, 64.0, 4.0".
2. Step 4: both [clients](#def-client) show exactly one pig at X -1, Z 4. It stays on the same block for 60 seconds.
3. Step 5: after the rejoin, [Player B](#def-player-b) sees exactly one pig at X -1, Z 4.
4. Step 6: the reply is "Unknown entity type: minecraft:banana" in red. No entity appears.
5. Step 7: the server answers with the player list, so it is running.
6. The [server log](#def-server-log) contains no line with "ERROR" or "panicked".

### VMC-ATP-017: Apply wandering behavior to an entity

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that the summon command can attach the implemented random-walk behavior to a non-player entity.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [Player A](#def-player-a), [join](#def-join), [operator access](#def-operator-access).
- VMC-ATP-016 has passed acceptance.
- The [Player A](#def-player-a) account exists.
- [Operator access](#def-operator-access): start the server with VOID_EXAMPLE_OPERATORS=1 cargo run -p voidmc-example.

**Steps:**

1. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a).
2. Stand at X 0, Z 0 and face south. Double-press Space to fly, and fly up to Y 70.
3. Run /summon minecraft:pig -1 64 4 --wander.
4. Run /summon minecraft:pig -4 64 4.
5. Take a screenshot with F2.
6. Watch both pigs for 60 seconds.
7. Take a screenshot with F2.

**Expected results:**

1. Steps 3 and 4: each command replies "Summoned minecraft:pig at" with its position.
2. Step 6: the pig with --wander walks and changes direction at least once. After 60 seconds it is at least 3 blocks from X -1, Z 4, or out of sight.
3. Step 6: the pig without the flag stays at X -4, Z 4 and does not walk.
4. Step 7: compared with step 5, only the --wander pig has moved.
5. The [server log](#def-server-log) contains no line with "ERROR" or "panicked".
Note: Wander is the documented demo random walk. It has no gravity or block collision unless those flags are given.

### VMC-ATP-018: Apply gravity and block collisions

**Category:** Critical feature  
**ATP status:** Done

**Objective:** Confirm that opt-in non-player physics settles entities on blocks and prevents them from clipping through simple obstacles.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [Player A](#def-player-a), [join](#def-join), [operator access](#def-operator-access), [fresh world](#def-fresh-world), [tools](#def-tools).
- VMC-ATP-016 has passed acceptance.
- The [Player A](#def-player-a) account exists.
- A [fresh world](#def-fresh-world).
- The working tree has no changes.

**Steps:**

The arena has a stone floor at Y 110 from X 17 to 30 and Z 17 to 30.
Glass walls 3 blocks high stand on its edges. A stone rise 1 block high runs along Z 25.
1. At the repository root, run git apply tools/atp/patches/physics-arena.patch.
2. Run [VOID_EXAMPLE_OPERATORS](#def-operator-access)=1 cargo run -p voidmc-example.
3. Wait until the [server log](#def-server-log) contains "ATP arena ready".
4. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a).
5. Run /tp 18.5 111 18.5. Look up.
6. Run /summon minecraft:pig 23.5 180 21.5 --wander --gravity --block-checks.
7. Watch the pig fall and land.
8. Watch the pig for 120 seconds.
9. Stop the server with Ctrl+C, then run git apply -R tools/atp/patches/physics-arena.patch.
10. Rename the world/ directory, for example to world.after-atp-018.

**Expected results:**

1. Step 7: the pig falls for more than 1 second and lands on the stone floor.
2. Step 7: after landing, the pig's feet are on top of the floor. It does not sink into the floor or hover above it.
3. Step 8: when the pig reaches the rise at Z 25, it steps up onto the rise and walks on top of it.
4. Step 8: the pig never passes through a glass wall and never leaves the arena.
5. Step 8: the pig does not shake, jump without a reason or disappear.
6. The [server log](#def-server-log) contains no line with "ERROR" or "panicked".

**ATP feedback:** Acceptance criteria aligned with the implemented one-block step-up behavior.

### VMC-ATP-019: Stream chunks around a moving player

**Category:** Critical feature  
**ATP status:** In progress

**Objective:** Confirm that chunk loading and unloading follow the player's effective view distance while the player crosses chunk boundaries.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [tester terminal](#def-tester-terminal), [client](#def-client), [Player A](#def-player-a), [join](#def-join), [F3 screen](#def-f3-screen), [tools](#def-tools).
- VMC-ATP-003 has passed acceptance.
- The [Player A](#def-player-a) account exists.
- Python 3.10 or later.
- The server view distance is the default, 10 chunks. The example does not change it.
- In the [client](#def-client), Options > Video Settings > Render Distance is 12 chunks. The effective view distance is then min(12, 10) = 10.

**Steps:**

1. At the repository root, run cargo run -p voidmc-example.
2. Wait until the [server log](#def-server-log) contains "Generated 81 spawn area chunks".
3. In the [tester terminal](#def-tester-terminal), run python3 tools/atp/capture-proxy.py --listen 127.0.0.1:25566 --upstream 127.0.0.1:25565 --out atp-019.
4. [Join](#def-join) 127.0.0.1:25566 with [Player A](#def-player-a). Port 25566 makes the capture proxy record the session.
5. Press [F3](#def-f3-screen). Read the Chunk line.
6. Double-press Space to fly.
7. Turn until the F3 Facing line shows "south (Towards positive Z)".
8. Fly forward until the F3 XYZ line shows Z of 128 or more. Watch the terrain ahead.
9. Turn until the F3 Facing line shows "north (Towards negative Z)".
10. Fly forward until the F3 XYZ line shows Z of 0 or less. Watch the terrain ahead.
11. Press Esc, then select Disconnect.
12. In the tester terminal, press Ctrl+C to stop the capture proxy.
13. Find the "closed" line with the most bytes. Record its conn-N number. Other lines are server-list pings.
14. Run python3 tools/atp/chunk-order.py --radius 10 atp-019/conn-N-server-to-[client](#def-client).bin, with N from step 13.

**Expected results:**

Definitions: the loaded set is the chunks the server sent and did not unload. It must be the square of chunks with max(|dx|, |dz|) ≤ 10 around the player's chunk: 441 chunks. The order uses the squared distance dx² + dz² from the player's chunk when the chunk is sent.
1. Step 5: the Chunk line shows chunk 0 0 (the first and last of the three numbers after "in").
2. Steps 8 and 10: terrain appears ahead of the player. No hole remains when you stop for 5 seconds.
3. Step 14: the tool prints "effective view distance: 10".
4. Step 14: the tool prints "loaded set after join equals square of radius 10 (441 chunks): yes".
5. Step 14: the tool prints "loaded set at end equals square of radius 10: yes".
6. Step 14: the tool prints "chunks sent outside the view square of the current centre: 0".
7. Step 14: the tool prints "intervals ending with a chunk outside the view square: 0".
8. Step 14: the tool prints "chunks sent while loaded: 0" and "re-sent chunks with different content: 0".
9. Step 14: the tool prints "order inversions: 0". This includes the chunks sent at the [join](#def-join).
10. Step 14: the tool prints "result: PASS".
11. The [client](#def-client) stays connected until step 11. The [server log](#def-server-log) contains no line with "ERROR" or "panicked".
Known result at 0220a6b: results 9 and 10 fail with one inversion, chunk (4, 4) d2=32 sent before chunk (-5, 0) d2=25 (verification report §019, GitHub issue #236).

### VMC-ATP-020: Load chunks through the world loader hook

**Category:** Critical feature  
**ATP status:** Done

**Objective:** Confirm that the loader interface is consulted before generation and that the built-in persistence adapter loads stored chunks while missing chunks use the generator fallback.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [Player A](#def-player-a), [join](#def-join), [F3 screen](#def-f3-screen), [operator access](#def-operator-access), [fresh world](#def-fresh-world).
- The [Player A](#def-player-a) account exists.
- A [fresh world](#def-fresh-world).
- The example uses the voidmc-world-io persistence plugin (RegionChunkStore) with the world directory world/.
- [Operator access](#def-operator-access): start the server with VOID_EXAMPLE_OPERATORS=1 cargo run -p voidmc-example.

**Steps:**

1. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a).
2. Place glowstone on top of the block at X 0, Y 63, Z 3 (stored chunk 0, 0).
3. Wait 15 seconds, then run /stop.
4. List the files in world/region/ and its subdirectories.
5. At the repository root, run [VOID_EXAMPLE_OPERATORS](#def-operator-access)=1 cargo run -p voidmc-example.
6. Wait until the [server log](#def-server-log) contains "Generated 81 spawn area chunks".
7. Join 127.0.0.1:25565 with Player A.
8. Point at X 0, Y 64, Z 3. Read the [F3](#def-f3-screen) Targeted Block line.
9. Run /tp 800 100 800 (chunk 50, 50, never visited, so not stored).
10. Wait 10 seconds and look at the terrain.

**Expected results:**

1. Step 4: there is at least one region file named r.0.0.vrm.
2. Step 8: the [F3](#def-f3-screen) Targeted Block line shows 0, 64, 3 and minecraft:glowstone. The generator does not create glowstone, so the chunk came from the store.
3. Step 10: terrain appears around the player in less than 10 seconds. This chunk was not stored, so the generator made it.
4. Steps 6 and 10: the [server log](#def-server-log) contains no line with "ERROR", "corrupt", "Failed to load" or "panicked".
Note: no external database backend is claimed. The loader hook itself (a custom ChunkLoader) is covered by the e2e scenario atp_020_world_loader.

### VMC-ATP-021: Register a custom command

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that an application or plugin can add a command without modifying the framework core.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [Player A](#def-player-a), [join](#def-join), [chat](#def-chat), [tools](#def-tools).
- The [Player A](#def-player-a) account exists.
- The example registers /hello with CommandBuilder::new("hello") in void-example/src/main.rs.
- The working tree has no changes.

**Steps:**

1. At the repository root, run cargo run -p voidmc-example.
2. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a).
3. In [chat](#def-chat), run /help. Look for the /hello line.
4. In chat, run /hello.
5. Stop the server with Ctrl+C.
6. Run git apply tools/atp/patches/remove-hello-command.patch.
7. Run cargo run -p voidmc-example.
8. Wait until the [server log](#def-server-log) contains "Generated 81 spawn area chunks".
9. Join 127.0.0.1:25565 with Player A.
10. In chat, run /help. Look for the /hello line.
11. In chat, run /hello.
12. In chat, run /ping.
13. Stop the server with Ctrl+C, then run git apply -R tools/atp/patches/remove-hello-command.patch.

**Expected results:**

1. Step 3: the list contains "/hello - Greet the player".
2. Step 4: the reply is "Hello from void-example!".
3. Step 7: the build and startup end without an error.
4. Step 10: the list does not contain /hello.
5. Step 11: the reply is "Unknown command: /hello" in red.
6. Step 12: the reply is "Pong!".

### VMC-ATP-022: Register a custom item behavior

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that an application can register an ItemBehavior for one item while unrelated items retain their default interaction behavior.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [Player A](#def-player-a), [join](#def-join), [chat](#def-chat), [F3 screen](#def-f3-screen).
- The [Player A](#def-player-a) account exists.
- The example registers WandBehavior for minecraft:stick (void-example/src/main.rs).
- The starter kit puts stone in hotbar slot 1 and a stick in hotbar slot 6.
- The server is started with cargo run -p voidmc-example. Players [join](#def-join) in Creative mode.

**Steps:**

1. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a).
2. Select hotbar slot 6 (stick).
3. Point at the top of the block at X 0, Y 63, Z 3. Use the [F3](#def-f3-screen) Targeted Block line.
4. Right-click.
5. Read the [chat](#def-chat). Point at X 0, Y 64, Z 3 and read the F3 Targeted Block line.
6. Select hotbar slot 1 (stone).
7. Point at the top of the block at X -2, Y 63, Z 3.
8. Right-click.
9. Point at X -2, Y 64, Z 3 and read the F3 Targeted Block line.

**Expected results:**

1. Step 5: the [chat](#def-chat) shows "zap! placed glowstone". [F3](#def-f3-screen) shows minecraft:glowstone at 0, 64, 3.
2. Step 5: hotbar slot 6 still holds the stick.
3. Step 8: no chat message appears.
4. Step 9: F3 shows minecraft:stone at -2, 64, 3.
5. The [server log](#def-server-log) contains no line with "ERROR" or "panicked".

### VMC-ATP-023: Extend the server through the Bevy plugin hook

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that an application can install a Bevy plugin closure that registers observers, systems, or shared ECS resources without modifying framework core code.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [Player A](#def-player-a), [join](#def-join), [chat](#def-chat), [tools](#def-tools).
- The [Player A](#def-player-a) account exists.
- The working tree has no changes.

**Steps:**

1. At the repository root, run git apply tools/atp/patches/event-observers.patch.
2. Run git diff --stat. Read the list of changed files.
3. Run cargo run -p voidmc-example.
4. Wait until the [server log](#def-server-log) contains "Generated 81 spawn area chunks".
5. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a).
6. In [chat](#def-chat), run /atpprobe.
7. Read the server log.
8. Stop the server with Ctrl+C, then run git apply -R tools/atp/patches/event-observers.patch.

**Expected results:**

1. Step 2: only void-example/src/main.rs is changed. No framework file and no Java or Spigot loader is involved.
2. Step 7: "ATP plugin applied" comes before "ATP event startup" and before the first "Accepted new connection" line.
3. Step 7: the "ATP event startup" line contains "ATP plugin resource". A Startup system read the resource.
4. Step 7: the "ATP event join" line contains "ATPPlayerA" and "ATP plugin resource". The observer ran and read the resource.
5. Step 6: the reply is "ATP resource: ATP plugin resource".
6. The [server log](#def-server-log) contains no line with "ERROR" or "panicked".

### VMC-ATP-024: Configure the server through ServerConfigBuilder

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that supported runtime parameters can be supplied through ServerConfigBuilder and are reflected in observable startup or status behavior.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [tester terminal](#def-tester-terminal), [client](#def-client), [Player A](#def-player-a), [join](#def-join), [tools](#def-tools).
- The [Player A](#def-player-a) account exists. In the [client](#def-client), Render Distance is 12 chunks.
- Python 3.10 or later.
- The working tree has no changes.

**Steps:**

The patch sets spawn_chunk_radius 1, initial_chunk_radius 1, view_distance 3, tick_rate 10, motd "ATP configuration check" and max_players 7.
1. At the repository root, run git apply tools/atp/patches/config-values.patch.
2. Run VOID_METRICS_DEBUG=1 VOID_TPS_OUTPUT=logs/atp-024-tps.csv cargo run -p voidmc-example.
3. Wait 15 seconds. Read the [server log](#def-server-log).
4. In the [tester terminal](#def-tester-terminal), run python3 tools/atp/status-ping.py --server 127.0.0.1:25565.
5. In the tester terminal, run python3 tools/atp/capture-proxy.py --out atp-024.
6. [Join](#def-join) 127.0.0.1:25566 with [Player A](#def-player-a). Walk 40 blocks in one direction.
7. Disconnect. Stop the proxy with Ctrl+C.
8. Find the "closed" line with the most bytes. Run python3 tools/atp/chunk-order.py --radius 3 atp-024/conn-N-server-to-[client](#def-client).bin with its N.
9. Stop the server with Ctrl+C. Open logs/atp-024-tps.csv.
10. Run git apply -R tools/atp/patches/config-values.patch.

**Expected results:**

1. Step 3: the [server log](#def-server-log) contains "Generated 9 spawn area chunks".
2. Step 4: the tool prints "motd: ATP configuration check" and "max players: 7".
3. Step 8: the tool prints "effective view distance: 3" and "loaded set at end equals square of radius 3: yes".
4. Step 9: after the first row, the tps column of logs/atp-024-tps.csv shows values between 9.5 and 10.5.
5. Step 10: git status shows no changes.
Note: max_players is advertised in the status only. It is not enforced (void-docs/docs/reference/server/configuration.md). No performance or capacity guarantee follows from these values.

### VMC-ATP-025: Capture TPS and profiling diagnostics

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that the example can emit TPS CSV samples and optional flame-compatible traces when diagnostics are explicitly enabled.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log).
- The working tree has no changes.
- The directory /proc/voidmc/ does not exist and cannot be created. It is used as an unavailable path.

**Steps:**

1. At the repository root, run VOID_METRICS_DEBUG=1 VOID_TPS_OUTPUT=logs/atp-025-tps.csv VOID_METRICS_MODE=flame VOID_FLAME_OUTPUT=logs/atp-025.folded cargo run -p voidmc-example.
2. Wait until the [server log](#def-server-log) contains "Generated 81 spawn area chunks".
3. Wait 10 seconds.
4. Press Ctrl+C. Wait until the process ends.
5. Read the first 3 lines of logs/atp-025-tps.csv.
6. Run wc -c logs/atp-025.folded.
7. Run VOID_METRICS_DEBUG=1 VOID_TPS_OUTPUT=/proc/voidmc/tps.csv VOID_METRICS_MODE=flame VOID_FLAME_OUTPUT=/proc/voidmc/trace.folded cargo run -p voidmc-example.
8. Wait 15 seconds. Read the server log.
9. Press Ctrl+C.

**Expected results:**

1. Step 4: the [server log](#def-server-log) contains "world persistence: flushed on shutdown" and the process ends in less than 10 seconds.
2. Step 5: the first line is "timestamp_ms,tps,window_ms,last_tick_ms,total_ticks".
3. Step 5: there are at least 2 more lines. Each starts with a Unix time in milliseconds.
4. Step 6: the size of logs/atp-025.folded is more than 0 bytes.
5. Step 8: the server log contains a WARN line "Cannot write the flame trace; flame profiling is off" with the path /proc/voidmc/trace.folded.
6. Step 8: the server log contains a WARN line "Failed to create TPS output directory; TPS samples are only logged" with the path /proc/voidmc/tps.csv.
7. Step 8: the server keeps running. The server log contains no line that says the trace or the CSV was written.

### VMC-ATP-026: Stop the server gracefully

**Category:** Critical feature  
**ATP status:** Done

**Objective:** Confirm that /stop and the supported process signals finish the current work and shut down without leaving dirty chunks unflushed.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [tester terminal](#def-tester-terminal), [Player A](#def-player-a), [join](#def-join), [chat](#def-chat), [F3 screen](#def-f3-screen), [operator access](#def-operator-access), [fresh world](#def-fresh-world), [tools](#def-tools).
- VMC-ATP-001 has passed acceptance.
- The [Player A](#def-player-a) account exists.
- A [fresh world](#def-fresh-world).
- The working tree has no changes.

**Steps:**

1. At the repository root, run git apply tools/atp/patches/shutdown-flush-only.patch.
2. Run cargo build -p voidmc-example.
3. Run [VOID_EXAMPLE_OPERATORS](#def-operator-access)=1 ./target/debug/voidmc-example; echo "exit=$?".
4. Wait until the [server log](#def-server-log) contains "Generated 81 spawn area chunks".
5. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a).
6. Place glowstone on top of the block at X 0, Y 63, Z 3.
7. In the [tester terminal](#def-tester-terminal), run pkill -TERM -x voidmc-example.
8. Read the terminal of step 3.
9. Repeat step 3. Join with Player A and point at X 0, Y 64, Z 3.
10. Place glass on top of the block at X -1, Y 63, Z 3.
11. In [chat](#def-chat), run /stop. Read the terminal of step 3.
12. Repeat step 3. Join and point at X -1, Y 64, Z 3.
13. Place stone on top of the block at X -2, Y 63, Z 3. In the server terminal, press Ctrl+C.
14. Repeat step 3. Join and point at X -2, Y 64, Z 3.
15. Stop the server with Ctrl+C, then run git apply -R tools/atp/patches/shutdown-flush-only.patch.

**Expected results:**

1. Steps 7, 11 and 13: the [server log](#def-server-log) contains "world persistence: flushed on shutdown" with "saved=1" (the count can be higher).
2. Steps 7 and 13: the terminal prints "exit=130".
3. Step 11: the terminal prints "exit=0".
4. Steps 9, 12 and 14: the [F3](#def-f3-screen) Targeted Block line shows minecraft:glowstone, minecraft:glass and minecraft:stone at the given positions.
5. Steps 7, 11 and 13: the process ends in less than 10 seconds. The server log contains no line with "ERROR" or "panicked".
6. Steps 6, 10 and 13: the server log contains no "World saved" line between the change and the stop. Only the shutdown flush saved the change.

### VMC-ATP-027: Generate optimized worlds

**Category:** Critical feature  
**ATP status:** Done

**Objective:** Confirm that the default world-generation pipeline (spawn-area pre-generation, chunk cache and per-tick generation budget) produces the expected terrain and spawn area for a defined generator fixture.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [Player A](#def-player-a), [join](#def-join), [F3 screen](#def-f3-screen), [fresh world](#def-fresh-world), [tools](#def-tools).
- VMC-ATP-026 has passed acceptance.
- The [Player A](#def-player-a) account exists.
- A [fresh world](#def-fresh-world).
- The working tree has no changes.
- The fixture is tools/atp/patches/striped-generator.patch. Its surface is at Y 80.
- Its top blocks are gold where world X mod 8 is 0 to 3, and grass where it is 4 to 7.

**Steps:**

1. At the repository root, run git apply tools/atp/patches/striped-generator.patch.
2. Run cargo run -p voidmc-example.
3. Wait until the [server log](#def-server-log) contains "Generated 81 spawn area chunks".
4. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a).
5. Press [F3](#def-f3-screen). Read the XYZ line.
6. Look down. Point at the top blocks at X 0, 3, 4 and 7, Z 0. Read the F3 Targeted Block line for each.
7. Turn until the F3 Facing line shows "east (Towards positive X)".
8. Walk forward until the F3 XYZ line shows X of 40 or more. Watch the ground.
9. Point at the top blocks at X 16, 19, 20 and 23, Z 0. Read the F3 Targeted Block line for each.
10. Walk back to X 0.
11. Stop the server with Ctrl+C, then run git apply -R tools/atp/patches/striped-generator.patch.
12. Rename the world/ directory, for example to world.after-atp-027.

**Expected results:**

1. Step 3: the [server log](#def-server-log) contains "Generated 81 spawn area chunks" and no line with "ERROR" or "panicked".
2. Step 5: the XYZ line shows Y 80.0. The player stands on the surface.
3. Step 6: X 0 and 3 are minecraft:gold_block. X 4 and 7 are minecraft:grass_block. All are at Y 79.
4. Step 8: the stripes continue without a gap, a step or a missing column at X 16, the chunk boundary.
5. Step 9: X 16 and 19 are minecraft:gold_block. X 20 and 23 are minecraft:grass_block. All are at Y 79.
6. Step 10: the ground on the way back is the same as on the way out.

### VMC-ATP-028: Process regions and systems in parallel

**Category:** Critical feature  
**ATP status:** Done

**Objective:** Confirm that independent regions and systems are processed concurrently without corrupting the world state.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [tools](#def-tools).
- VMC-ATP-027 has passed acceptance.
- Python 3.10 or later.
- A computer with at least 2 CPU cores (nproc prints 2 or more).
- The working tree has no changes.
- Scope: the region-partitioned entity physics step. The unordered system pairs listed in void-docs/docs/reference/server/configuration.md are out of scope.

**Steps:**

The patch spawns 60 pigs in region (0,0) and 60 in region (-1,-1) from a fixed seed. It records 120 ticks.
1. At the repository root, run git apply tools/atp/patches/region-parallel.patch.
2. Run cargo build -p voidmc-example.
3. Run ATP_WORLD_DIR=$(mktemp -d) ATP_TRACE_OUT=atp-028-serial ./target/debug/voidmc-example.
4. Wait until the [server log](#def-server-log) contains "ATP trace complete, 120 ticks", then press Ctrl+C.
5. Run ATP_EXECUTION=multi ATP_WORLD_DIR=$(mktemp -d) ATP_TRACE_OUT=atp-028-multi ./target/debug/voidmc-example.
6. Wait until the server log contains "ATP trace complete, 120 ticks", then press Ctrl+C.
7. Run python3 tools/atp/region-trace-analyze.py --serial atp-028-serial --multi atp-028-multi.
8. Run git apply -R tools/atp/patches/region-parallel.patch.

**Expected results:**

1. Step 3: the [server log](#def-server-log) contains "Game tick execution mode" with "SingleThreaded".
2. Step 5: the server log contains "Game tick execution mode" with "MultiThreaded".
3. Step 7: both runs show ticks=120, contiguous=yes, wrong_entity_count=0 and region_twice=0.
4. Step 7: the serial run shows ticks_with_several_threads=0 and ticks_with_overlap=0.
5. Step 7: the multi run shows ticks_with_overlap of 60 or more. Two regions ran at the same time on these ticks.
6. Step 7: the tool prints "trace rows: 14400", "traces identical: yes" and "result: PASS".
7. Steps 4 and 6: the server log contains no line with "ERROR" or "panicked", and each run ends within 60 seconds.

### VMC-ATP-029: Simulate biomes and weather

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that configured biome and weather states are applied to a test area and synchronized to the client.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [client](#def-client), [Player A](#def-player-a), [Player B](#def-player-b), [join](#def-join), [F3 screen](#def-f3-screen).
- VMC-ATP-027 has passed acceptance.
- The [Player A](#def-player-a) and [Player B](#def-player-b) accounts exist.
- The server is started with cargo run -p voidmc-example.
- On both [clients](#def-client), the [F3](#def-f3-screen) screen shows the biome line. To enable it, press F3+F6, set "biome" to "In Overlay", then click Done.

**Steps:**

1. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a). Stand at X 0, Z 0.
2. With Player A, run /biome void_example:crimson_sky 4.
3. Read the [F3](#def-f3-screen) biome line, then walk to X 30, Z 0 and read it again.
4. With Player A, stand at X 30, Z 0 and run /weather thunder 40.
5. Wait 5 seconds. Look at the sky and the ground.
6. Walk to X 0, Z 0. Look at the sky and the ground.
7. Run /weather clear 40. Wait 5 seconds.
8. Run /weather thunder 600. Wait 15 seconds.
9. Join with [Player B](#def-player-b). Move Player B to X 30, Z 0 next to Player A.
10. Compare the rain on both [clients](#def-client) for 20 seconds.
11. With Player A, run /weather clear 40. Watch both clients for 5 seconds.

**Expected results:**

1. Step 2: the reply starts with "Set 729 cell(s) to void_example:crimson_sky".
2. Step 3: at X 0 the biome line shows void_example:crimson_sky and the sky is red. At X 30 it shows minecraft:plains.
3. Step 4: the reply starts with "Weather set to Thunder".
4. Step 5: at X 30 rain falls and the sky is dark.
5. Step 6: at X 0 no rain falls (the biome has no precipitation), and the sky is darker than before step 4.
6. Step 7: the rain stops and the sky is clear again.
7. Step 10: both [clients](#def-client) show rain at the same time, with the same density.
8. Step 11: the rain stops on both clients within 5 seconds.
Note: a weather cycle, lightning, snow and biome-driven behaviour are not claimed (void-docs).

### VMC-ATP-030: Trigger note blocks with basic redstone

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that basic redstone behavior activates note blocks.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [client](#def-client), [Player A](#def-player-a), [join](#def-join), [chat](#def-chat).
- VMC-ATP-027 has passed acceptance.
- The [Player A](#def-player-a) account exists.
- The server is started with cargo run -p voidmc-example. The example enables RedstonePlugin.
- The [client](#def-client) sound is on (Options > Music & Sounds > Blocks is more than 0%).

**Steps:**

The circuit is a lever, 3 redstone dust and a note block in a line towards positive X.
It starts 2 blocks east of the player.
1. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a). Stand at X 0, Z 0.
2. In [chat](#def-chat), run /circuit.
3. Look at the circuit east of the player.
4. Right-click the lever.
5. Right-click the lever again.
6. Right-click the lever a third time.
7. Disconnect, then join again with Player A.
8. Look at the circuit.
9. Right-click the lever, then right-click it again.

**Expected results:**

1. Step 2: the reply starts with "Circuit built at" and the lever, dust and note block appear.
2. Step 4: the lever turns down, the dust turns bright red, and the note block shows a note particle and plays a harp note.
3. Step 5: the lever turns up and the dust turns dark.
4. Step 6: the dust turns bright red again and the note block shows a note particle and plays a note.
5. Step 8: the lever is down and the dust is bright red, as before step 7.
6. Step 9: the first click turns the dust dark. The second click turns it red, with a note particle and sound.
Note: repeaters, comparators, pistons and complex circuits are out of scope.

### VMC-ATP-032: Expose health and Prometheus metrics

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that the server exposes a health endpoint and Prometheus-compatible metrics for a running server.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [tester terminal](#def-tester-terminal), [Player A](#def-player-a), [join](#def-join), [chat](#def-chat), [tools](#def-tools).
- VMC-ATP-027 has passed acceptance.
- The [Player A](#def-player-a) account exists.
- curl and Python 3.10 or later.
- TCP port 9464 is free.
- The agreed metrics are the 14 families in void-docs/docs/reference/server/observability.md, section /metrics.
- The working tree has no changes.

**Steps:**

1. At the repository root, run git apply tools/atp/patches/stall-command.patch.
2. Run cargo build -p voidmc-example.
3. In the [tester terminal](#def-tester-terminal), run python3 tools/atp/health-poll.py --address 127.0.0.1:9464 --seconds 300.
4. Run VOID_OBSERVABILITY_ADDR=9464 ./target/debug/voidmc-example.
5. Wait until the [server log](#def-server-log) contains "Generated 81 spawn area chunks".
6. In a third terminal, run curl -i http://127.0.0.1:9464/health.
7. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a).
8. Run python3 tools/atp/metrics-check.py --address 127.0.0.1:9464.
9. In [chat](#def-chat), run /atpstall. Wait 10 seconds.
10. In the server terminal, press Ctrl+C. Wait 5 seconds.
11. Run curl -i http://127.0.0.1:9464/health.
12. Stop health-poll.py with Ctrl+C. Run git apply -R tools/atp/patches/stall-command.patch.

**Expected results:**

1. Step 6: the status is "HTTP/1.1 200" and the body contains "status":"ok".
2. Step 8: the tool prints "missing families: none", "undocumented families: none", "format errors: 0" and "result: PASS".
3. Step 8: the tool prints "voidmc_players_online: 1" and "voidmc_players_max: 100".
4. Step 9: health-poll.py prints "HTTP 503 stalled" and then "HTTP 200 ok".
5. Step 12: before step 9, health-poll.py printed "refused", "HTTP 503 starting" and "HTTP 200 ok", in this order.
6. Step 12: after step 10, health-poll.py printed "refused".
7. Step 12: a "HTTP 503 stopping" line just before "refused" is accepted but not required. The state can last less than 5 ms.
8. Step 11: curl prints "Connection refused" (exit status 7).

### VMC-ATP-033: Advertise commands and complete command suggestions

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that the server advertises its command tree and returns controlled tab-completion suggestions for supported inputs.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [client](#def-client), [Player A](#def-player-a), [Player B](#def-player-b), [join](#def-join), [chat](#def-chat), [operator access](#def-operator-access).
- VMC-ATP-003 has passed acceptance.
- The [Player A](#def-player-a) and [Player B](#def-player-b) accounts exist.
- [Operator access](#def-operator-access): start the server with VOID_EXAMPLE_OPERATORS=1 cargo run -p voidmc-example. /summon is offered only to operators.

**Steps:**

1. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a), then with [Player B](#def-player-b).
2. With Player A, open [chat](#def-chat) and type /he. Do not press Enter. Read the suggestions.
3. Clear the chat line. Type /tell ATP and read the suggestions.
4. Clear the chat line. Type /summon minecraft:pi and read the suggestions.
5. Clear the chat line. Type /zzzz and press Tab.
6. Clear the chat line. Type /gamemode and a space. Read the suggestions.
7. Press Esc. Wait 10 seconds.

**Expected results:**

1. Step 2: the suggestions are hello and help, and nothing else.
2. Step 3: the suggestions contain ATPPlayerA and ATPPlayerB.
3. Step 4: the suggestions contain minecraft:pig. Every suggestion starts with "minecraft:pi".
4. Step 5: no suggestion appears.
5. Step 6: the suggestions contain the four game modes: survival, creative, adventure and spectator.
6. Step 7: both [clients](#def-client) are still connected. The [server log](#def-server-log) contains no line with "ERROR" or "panicked".
Note: the client completes these suggestions from the command tree that the server sent at [join](#def-join).

### VMC-ATP-034: Add custom player behavior through a Bevy plugin

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that an application can attach a Bevy observer to player lifecycle events and react to them without modifying framework core code.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [client](#def-client), [Player A](#def-player-a), [join](#def-join), [tools](#def-tools).
- VMC-ATP-023 has passed acceptance.
- The [Player A](#def-player-a) account exists.
- The working tree has no changes.

**Steps:**

1. At the repository root, run git apply tools/atp/patches/event-observers.patch.
2. Run VOID_PACKET_DEBUG=1 cargo run -p voidmc-example.
3. Wait until the [server log](#def-server-log) contains "Generated 81 spawn area chunks".
4. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a).
5. Wait 10 seconds after the world appears.
6. Press Esc, then select Disconnect.
7. Wait 5 seconds. Read the server log.
8. Join 127.0.0.1:25565 with Player A again, to check that the server still accepts players.
9. Stop the server with Ctrl+C, then run git apply -R tools/atp/patches/event-observers.patch.

**Expected results:**

1. Step 7: the lines for ATPPlayerA and its [client](#def-client) number occur in this order:
   - "Received FinishConfigurationAcknowledged"
   - "ATP event join"
   - "Received PlayerLoaded"
   - "ATP event ready"
   - "ATP event quit"
2. Step 7: the [join](#def-join), ready and quit lines contain "player=ATPPlayerA" and the same client_id as the [Client N] number.
3. Step 7: each of the join, ready and quit lines occurs once for this session.
4. Step 8: [Player A](#def-player-a) joins without an error.
5. The [server log](#def-server-log) contains no line with "ERROR" or "panicked".

### VMC-ATP-035: Observe gameplay events through a Bevy plugin

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that an application can observe implemented movement, chat, block, and item interaction events through Bevy observers.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [Player A](#def-player-a), [join](#def-join), [chat](#def-chat), [F3 screen](#def-f3-screen), [operator access](#def-operator-access), [tools](#def-tools).
- VMC-ATP-023 has passed acceptance.
- The [Player A](#def-player-a) account exists.
- [Operator access](#def-operator-access), for /give. The steps start the server with it.
- The working tree has no changes.

**Steps:**

1. At the repository root, run git apply tools/atp/patches/event-observers.patch.
2. Run [VOID_EXAMPLE_OPERATORS](#def-operator-access)=1 cargo run -p voidmc-example.
3. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a). Stand at X 0, Z 0.
4. Walk 3 blocks towards positive Z.
5. Turn until the [F3](#def-f3-screen) Facing line shows a yaw of about 90. Record the yaw and pitch.
6. In [chat](#def-chat), type ATP observer 2718 and press Enter.
7. Break the block at X 0, Y 63, Z 3.
8. Select stone (hotbar slot 1) and place it at X 0, Y 63, Z 3.
9. Run /give minecraft:ender_pearl 1. Select it and right-click into the air.
10. Read the [server log](#def-server-log).
11. Stop the server with Ctrl+C, then run git apply -R tools/atp/patches/event-observers.patch.

**Expected results:**

1. Step 4: the [server log](#def-server-log) contains "ATP event move" lines with "player=ATPPlayerA". The new Z values increase towards 3.
2. Step 5: an "ATP event rotate" line has a yaw within 1 degree of the [F3](#def-f3-screen) yaw and the same pitch.
3. Step 6: an "ATP event chat" line contains "player=ATPPlayerA" and "text=ATP observer 2718".
4. Step 7: an "ATP event break" line contains "position=0,63,3".
5. Step 8: an "ATP event place" line contains "position=0,63,3" and "state=1" (stone).
6. Step 9: an "ATP event use-item" line contains "hand=MainHand".
7. No event line names a player other than ATPPlayerA. No combat or damage event is claimed.

### VMC-ATP-036: Provide a custom world generator

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that an application can replace the default terrain generator with a recognizable custom generator.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [server log](#def-server-log), [Player A](#def-player-a), [join](#def-join), [F3 screen](#def-f3-screen), [fresh world](#def-fresh-world), [tools](#def-tools).
- VMC-ATP-023 has passed acceptance.
- The [Player A](#def-player-a) account exists.
- A [fresh world](#def-fresh-world).
- The working tree has no changes.

**Steps:**

1. At the repository root, run git apply tools/atp/patches/striped-generator.patch.
2. Read the patch: it implements WorldGenerator and passes it to ServerConfigBuilder::world_generator.
3. Run cargo run -p voidmc-example.
4. Wait until the [server log](#def-server-log) contains "Generated 81 spawn area chunks".
5. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a). Read the [F3](#def-f3-screen) XYZ line.
6. Point at the top blocks at X 0 and X 4, Z 0. Read the F3 Targeted Block line.
7. Walk towards positive X until X is 24 or more.
8. Point at the top blocks at X 16 and X 20, Z 0.
9. Read the server log.
10. Stop the server with Ctrl+C, then run git apply -R tools/atp/patches/striped-generator.patch.
11. Rename the world/ directory, for example to world.after-atp-036.

**Expected results:**

1. Step 5: the XYZ line shows Y 80.0. No spawn height is set in ServerConfigBuilder, so the spawn uses the generator's surface height.
2. Step 6: X 0 is minecraft:gold_block and X 4 is minecraft:grass_block, both at Y 79.
3. Step 8: X 16 is minecraft:gold_block and X 20 is minecraft:grass_block, both at Y 79.
4. Step 9: the [server log](#def-server-log) contains "ATP generator chunk" lines for chunk=0,0 and chunk=1,0.
5. The server log contains no line with "ERROR" or "panicked".

### VMC-ATP-037: Add a custom biome

**Category:** Secondary function  
**ATP status:** Done

**Objective:** Confirm that an application can add a custom biome with a clearly visible effect in a defined test area.

**Prerequisites:**

- Defined terms used here ([Definitions](#definitions)): [Player A](#def-player-a), [join](#def-join), [F3 screen](#def-f3-screen).
- VMC-ATP-023 has passed acceptance.
- The [Player A](#def-player-a) account exists.
- The example registers void_example:crimson_sky (red sky, red grass) in void-example/src/biome.rs.
- The server is started with cargo run -p voidmc-example.
- The [F3](#def-f3-screen) screen shows the biome line. To enable it, press F3+F6, set "biome" to "In Overlay", then click Done.

**Steps:**

The test area is the 9 x 9 x 9 biome cells around X 0, Y 64, Z 0. The biome is placed at runtime with /biome.
1. [Join](#def-join) 127.0.0.1:25565 with [Player A](#def-player-a). Stand at X 0, Z 0.
2. Run /biome void_example:crimson_sky 4.
3. Run /biome.
4. Read the [F3](#def-f3-screen) biome line. Look at the sky and the grass.
5. Run /tp 100 80 100. Read the F3 biome line.
6. Run /tp 0 64 0. Read the F3 biome line.
7. Disconnect, then join again. Read the F3 biome line.

**Expected results:**

1. Step 2: the reply starts with "Set 729 cell(s) to void_example:crimson_sky".
2. Step 3: the reply is "You are in void_example:crimson_sky" with its id.
3. Step 4: the biome line shows void_example:crimson_sky. The sky and the grass are red.
4. Step 5: the biome line shows minecraft:plains. The sky and the grass have their normal colours.
5. Step 6: the biome line shows void_example:crimson_sky again, with the red sky and grass.
6. Step 7: the biome line still shows void_example:crimson_sky.
