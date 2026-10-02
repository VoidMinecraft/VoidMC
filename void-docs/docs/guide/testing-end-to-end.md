# Testing your server end to end

Unit tests check one system at a time. The end-to-end harness, `voidmc-e2e`,
checks what a player actually experiences: it starts a real VoidMC server
in the test process, connects real Minecraft clients over TCP, and lets the
test drive them packet by packet through login, configuration and play.

The clients are built on [Azalea](https://github.com/azalea-rs/azalea)'s
protocol crate (`azalea-protocol 0.16`, protocol 775), an implementation
written independently from VoidMC. Every packet the server sends is decoded by
Azalea with strict length checks, and chunks are parsed section by section, so
a packet the vanilla client could not read fails the test even if VoidMC's own
decoder accepts it.

## Running the scenarios

Azalea needs a nightly compiler, so the harness is its own Cargo workspace
with a pinned toolchain (`void-e2e/rust-toolchain.toml`). The rest of the
repository stays on stable; `cargo test --workspace` at the root does not build
it.

```bash
cd void-e2e
cargo test
```

The first build takes about a minute: dependencies are compiled with
optimisations so the suite runs in seconds. CI runs the same command in the
`e2e` job.

Set `VOID_E2E_LOG` to an `EnvFilter` directive to see the server logs:

```bash
VOID_E2E_LOG=voidmc=debug cargo test -- --nocapture block_changes
```

## Writing a scenario

```rust
use voidmc_e2e::TestServer;
use voidmc_e2e::core::position::Vec3;
use voidmc_e2e::protocol::packets::game::ClientboundGamePacket;

#[tokio::test]
async fn teleport_is_seen_by_others() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;

    alice.command("tp 10 100 10").await;
    alice
        .expect("the teleport", |packet| match packet {
            ClientboundGamePacket::PlayerPosition(teleport) => Some(teleport.change.pos),
            _ => None,
        })
        .await;

    let destination = Vec3::new(10.0, 100.0, 10.0);
    bob.wait_until("Alice teleported", |view| {
        view.entity_by_uuid(alice.uuid())
            .is_some_and(|(_, entity)| entity.position == destination)
    })
    .await;

    alice.assert_healthy();
    server.stop().await;
}
```

### The server

`TestServer::builder()` starts from a small, deterministic configuration:
`127.0.0.1:0`, 50 ticks per second, view distance 4, spawn radius 4, the
default world generator and the default commands. Change it with:

| Method | Effect |
|---|---|
| `config(\|c\| c.view_distance(6))` | Edits the `ServerConfigBuilder` |
| `compression_threshold(Some(64))` | Shortcut for the compression setting |
| `plugin(\|app\| { ... })` | Adds systems, observers or resources, like `VoidServer::add_plugin` |
| `command(my_command)` | Registers a command built by `my_command()` |
| `without_default_commands()` | Starts with an empty command registry |

On a running server:

- `join(name)` / `join_with(name, BotOptions { view_distance })` log a bot in
  and return once the server lists it, so it is ready to play.
- `with_world(|world| ...)` runs a closure on the game thread at the start of
  the next tick and returns its result. Use it to read or change server state.
- `wait_for(what, |world| Option<T>)` polls once per tick until the closure
  returns `Some`.
- `wait_ticks(n)`, `status()` (the server-list ping), `player_entity(uuid)`.
- `stop()` shuts the server down and fails the test if VoidMC logged an
  error, or dropped a packet it did not recognise, on any thread.

Only one `TestServer` runs at a time in a test process: the others wait for it
to stop. That is what lets `stop()` attribute every log line to the right
server.

### The bots

A `Bot` answers keep-alives and pings, accepts teleports (then sends its
position and `PlayerLoaded`, like the vanilla client) and keeps a
`ClientView`: its position, the chunks it holds, the entities and players it
knows and its inventory. Everything else is up to the test:

- `send(packet)` sends any serverbound play packet; `command`, `chat` and
  `move_to` are shortcuts.
- `expect(what, matcher)` removes and returns the oldest received packet the
  matcher accepts, waiting for it if needed. Packets that do not match stay
  queued, so the order of expectations does not have to follow the order of
  packets.
- `wait_until(what, |view| bool)` waits for the client view to reach a state.
- `queued(matcher)` and `clear()` inspect or drop queued packets.
- `assert_healthy()` fails on decode errors, kicks, and protocol anomalies a
  vanilla client tolerates but that point at a server bug: an entity added
  twice, a block update for a chunk the client does not hold, a forget for a
  chunk it never received.
- `disconnect()` closes the connection like a player quitting.

### Waiting without sleeping

Never sleep to let the server catch up. Wait for something observable instead:

- `expect` and `wait_until` wait for what the client receives.
- `wait_for` waits for what the server holds.
- `bot.sync()` returns once the server has handled every packet the bot sent
  before the call, finished that tick, and every packet it sent the bot during
  that tick has arrived. Use it before asserting that something did **not**
  happen, or before reading server state that the bot's packets changed.

Every wait times out after 15 seconds with a message naming what it waited
for and, for `expect`, the packets received so far.

`sync()` works by sending a `Pong` with the high bit set; the harness answers
it with a `Ping` carrying the same id at the end of the tick. Pongs without
that bit are left to the server as usual.

## What the scenarios cover

The scenarios in `void-e2e/tests/scenarios/` were checked against deliberately
broken servers: each of these regressions makes at least one of them fail.

| Regression | Caught by |
|---|---|
| Chunk section header off by two bytes | every scenario |
| Compression announced but not enabled | every scenario with compression |
| Chunks no longer forgotten behind a moving player | `chunks_follow_the_player_and_are_forgotten_behind_it` |
| Block change broadcast to the wrong chunk | `block_changes_reach_other_players_and_late_joiners` |
| Block change broadcast but not stored | `block_changes_reach_other_players_and_late_joiners` |
| Keep-alive answers never accepted | `keep_alive_round_trips_and_keeps_going` |
| Wrong serverbound packet id for container clicks | `clicks_move_items_between_slots` |
| Suggestion ranges in bytes instead of UTF-16 units | `server_suggestions_use_utf16_ranges` |
| Joining player not announced to others | `players_see_each_other_move_and_leave` |
| Quitting player not removed for others | `players_see_each_other_move_and_leave` |
