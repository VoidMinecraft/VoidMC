# Testing Guide

Comprehensive testing is essential for maintaining code quality and preventing regressions. This guide explains how to write and run tests in Void.

## Test Organization

Tests are organized into four categories:

### Unit Tests

Located within the source code files, right next to the code they test.

```rust
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add() {
        assert_eq!(add(2, 3), 5);
    }
}
```

### Integration Tests

Located in the `tests/` directory, testing the public API.

```
crate/
├── src/
│   ├── lib.rs
│   └── main.rs
└── tests/
    ├── common.rs
    ├── integration_test_1.rs
    └── integration_test_2.rs
```

### Documentation Tests

Tests embedded in documentation comments.

````rust
/// Adds two numbers.
///
/// # Examples
///
/// ```
/// use voidmc::add;
/// assert_eq!(add(2, 3), 5);
/// ```
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
````

### End-to-End Tests

Located in `void-e2e/`, a separate Cargo workspace. They start a real server in
the test process and connect real Minecraft clients to it over TCP. See
[End-to-End Tests](#end-to-end-tests) below.

## Running Tests

### Basic Commands

```bash
# Run all tests
cargo test

# Run tests for a specific crate (package names start with `voidmc-`)
cargo test -p voidmc-codec

# Run specific test by name
cargo test test_varint

# Run with output visible
cargo test -- --nocapture

# Run single-threaded (easier debugging)
cargo test -- --test-threads=1

# Run ignored tests only
cargo test -- --ignored

# Run all including ignored
cargo test -- --include-ignored
```

### Advanced Options

```bash
# Show test names without running
cargo test --list

# Run with environment variables
RUST_BACKTRACE=full cargo test

# Generate coverage (the CI tool)
cargo llvm-cov --workspace --all-features

# Benchmark
cargo bench
```

## Writing Good Tests

### Principles

1. **Arrange-Act-Assert Pattern**

```rust
#[test]
fn test_decode_packet() {
    // Arrange
    let mut buffer = vec![0x03, 0x00, 0x01];

    // Act
    let result = Packet::decode(&mut buffer);

    // Assert
    assert!(result.is_ok());
}
```

2. **One Assertion Per Test** (when possible)

```rust
// Good: focused test
#[test]
fn test_player_name_is_stored() {
    let player = Player::new("Alice".to_string());
    assert_eq!(player.name, "Alice");
}

// Also good: multiple related assertions
#[test]
fn test_player_creation() {
    let player = Player::new("Alice".to_string());
    assert_eq!(player.name, "Alice");
    assert_eq!(player.position, Position::default());
}

// Less good: testing multiple independent things
#[test]
fn test_everything() {
    // player tests
    // entity tests
    // world tests
}
```

3. **Descriptive Names**

```rust
// Good
#[test]
fn decode_varint_with_value_zero_returns_zero() { }

#[test]
fn encode_string_with_empty_input_produces_zero_length() { }

// Less clear
#[test]
fn test_varint() { }

#[test]
fn test_string() { }
```

### Example Test Suite

```rust
#[cfg(test)]
mod tests {
    use super::*;

    mod encode {
        use super::*;

        #[test]
        fn varint_zero() {
            let mut buf = Vec::new();
            0i32.encode(&mut buf);
            assert_eq!(buf, vec![0x00]);
        }

        #[test]
        fn varint_127() {
            let mut buf = Vec::new();
            127i32.encode(&mut buf);
            assert_eq!(buf, vec![0x7f]);
        }

        #[test]
        fn varint_128() {
            let mut buf = Vec::new();
            128i32.encode(&mut buf);
            assert_eq!(buf, vec![0x80, 0x01]);
        }
    }

    mod decode {
        use super::*;

        #[test]
        fn varint_single_byte() {
            let mut buf = &[0x05][..];
            let value = i32::decode(&mut buf).unwrap();
            assert_eq!(value, 5);
        }

        #[test]
        fn varint_overflow_returns_error() {
            let mut buf = &[0xFF, 0xFF, 0xFF, 0xFF, 0xFF][..];
            let result = i32::decode(&mut buf);
            assert!(result.is_err());
        }
    }
}
```

## Testing Different Components

### Codec Testing (void-codec)

Focus on serialization/deserialization:

```rust
#[test]
fn roundtrip_packet_data() {
    let original = SomePacket { /* ... */ };

    // Encode
    let mut buffer = Vec::new();
    original.encode(&mut buffer);

    // Decode
    let mut reader = &buffer[..];
    let decoded = SomePacket::decode(&mut reader).unwrap();

    // Verify
    assert_eq!(original, decoded);
}
```

### Protocol Testing (void-protocol)

Test packet definitions and state transitions:

```rust
#[test]
fn play_state_accepts_move_packet() {
    let packet = PlayPacket::PlayerMovement { /* ... */ };
    let state = State::Play;

    assert!(is_valid_packet_for_state(&packet, state));
}

#[test]
fn status_state_rejects_play_packets() {
    let packet = PlayPacket::PlayerMovement { /* ... */ };
    let state = State::Status;

    assert!(!is_valid_packet_for_state(&packet, state));
}
```

### Async Testing

Use `#[tokio::test]` for async code:

```rust
#[tokio::test]
async fn client_receives_welcome_message() {
    let server = start_test_server().await;
    let mut client = connect_to_server(&server).await;

    let message = client.read_next_packet().await.unwrap();
    assert_eq!(message.text, "Welcome to Void!");
}

#[tokio::test]
async fn concurrent_clients_dont_interfere() {
    let server = start_test_server().await;

    let clients: Vec<_> = (0..10)
        .map(|_| connect_to_server(&server))
        .collect();

    // Verify all are connected
    assert_eq!(clients.len(), 10);
}
```

## End-to-End Tests

Unit and integration tests check VoidMC with VoidMC's own types. The
end-to-end suite, `voidmc-e2e`, checks what a player actually experiences: it
starts a real `VoidServer` in the test process on an ephemeral port, connects
Minecraft clients over TCP, and drives them packet by packet through login,
configuration and play.

The clients are built on [Azalea](https://github.com/azalea-rs/azalea)'s
protocol crate, a Rust implementation written independently from VoidMC.
Every packet the server sends is decoded by Azalea, which rejects leftover
bytes, and chunks are parsed section by section. A bug shared by VoidMC's
encoder and its own decoder, which a round-trip test cannot see, fails here.
The reasons for this choice are in
[Technology Evaluations](/architecture/technology-evaluations#azalea-for-end-to-end-tests).

### Running the Suite

Azalea needs a nightly compiler, so `void-e2e/` is its own Cargo workspace
with a pinned toolchain in `void-e2e/rust-toolchain.toml`. The root workspace
excludes it: `cargo test --workspace` does not build it.

```bash
cd void-e2e
cargo test
```

The first build takes a few minutes, because dependencies are compiled with
optimisations so that the suite runs in seconds. A `RUSTUP_TOOLCHAIN`
environment variable overrides `rust-toolchain.toml`; unset it if the build
complains about nightly features.

Set `VOID_E2E_LOG` to an `EnvFilter` directive to see the server logs:

```bash
VOID_E2E_LOG=voidmc=debug cargo test -- --nocapture block_changes
```

CI runs `cargo fmt --check`, `cargo clippy` and `cargo test` in this directory
in the `e2e` job, which `ci-ok` requires.

### Writing a Scenario

Scenarios live in `void-e2e/tests/scenarios/`, one module per area.

```rust
use voidmc_e2e::TestServer;
use voidmc_e2e::core::position::Vec3;
use voidmc_e2e::protocol::packets::game::ClientboundGamePacket;

#[tokio::test]
async fn teleport_is_seen_by_others() {
    let server = TestServer::builder().operators().start().await;
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

`TestServer::builder()` starts from a small configuration: `127.0.0.1:0`, 50
ticks per second, view distance 4, the default world generator and the default
commands.

| Method | Effect |
|---|---|
| `config(\|c\| c.view_distance(6))` | Edits the `ServerConfigBuilder` |
| `compression_threshold(Some(64))` | Shortcut for the compression setting |
| `plugin(\|app\| { ... })` | Adds systems, observers or resources, like `VoidServer::add_plugin` |
| `command(my_command)` | Registers a command built by `my_command()` |
| `without_default_commands()` | Starts with an empty command registry |
| `operators()` | Makes every player an `Operator` from login, so its bots may run the operator-only default commands (`/tp`, `/give`, `/kick`, `/stop`, ...) |
| `capture_logs(Level::DEBUG)` | Also records the events logged down to that level (by default only warnings and errors), for a scenario that checks what the server logs at info or debug level |

On a running server:

- `join(name)` and `join_with(name, BotOptions { view_distance })` log a bot
  in and return once the server lists it.
- `try_join(name)` does the same, but returns `Err(LoginRefused)` when the
  server refuses the login: its `reason` as the client shows it ("The server
  is full!") and its text `component` as JSON. The harness also checks that
  the server closed the connection after the refusal.
- `join_with_information(name, ClientInformation { .. })` joins like `join`,
  with that Client Information (language, view distance, chat visibility,
  ...) sent during configuration.
- `log_in(name)` logs a client in and leaves it in the configuration state,
  holding its connection without ever playing, until its `disconnect()`. Its
  `expect_disconnect()` waits for the server's configuration `Disconnect` and
  the close after it, and returns them as a `LoginRefused`.
- `join_silent(name)` plays a client into the play state, where it then never
  answers: no keep-alive, ping or teleport reply, and no `PlayerLoaded` until
  its `report_loaded()`. Its `expect(what, f)` reads packets until `f` matches
  one; its `expect_disconnect()` returns the play `Disconnect` reason once the
  server has closed the connection.
- `with_world(|world| ...)` runs a closure on the game thread at the start of
  the next tick and returns its result. That is never the first tick, so
  even right after `start()` the closure sees a world in which every system
  of the first tick has run.
- `wait_for(what, |world| Option<T>)` polls once per tick until the closure
  returns `Some`.
- `wait_ticks(n)`, `status()` (the server-list ping) and `player_entity(uuid)`.
- `op(&bot)` and `deop(&bot)` add and remove `Operator` on a connected bot's
  player, as an application would at runtime.
- `logs()` returns the events logged so far, on any thread (the bots' too):
  warnings and errors, or down to the level given to `capture_logs`.
- `stop()` shuts the server down and fails the test if VoidMC logged an
  error, or an unrecognised packet, on any thread.
- `wait_for_exit()` waits for the server to stop on its own, for example
  after a bot ran `/stop`, fails the test like `stop()`, and returns the
  `AppExit` that `VoidServer::run` returned.

Only one `TestServer` runs at a time in a test process; the others wait for it
to stop. That is what lets `stop()` attribute every log line to the right
server.

A `Bot` answers keep-alives and pings, accepts teleports like the vanilla
client, and keeps a `ClientView` of what a vanilla client would remember: its
position, chunks, entities, tab list and inventory.

- `send(packet)` sends any serverbound play packet; `command`, `chat` and
  `move_to` are shortcuts.
- `expect(what, matcher)` removes and returns the oldest received packet the
  matcher accepts, waiting for it if needed. Packets that do not match stay
  queued, so expectations do not have to follow the order of packets.
- `wait_until(what, |view| bool)` waits for the client view to reach a state.
- `queued(matcher)` and `clear()` inspect or drop queued packets.
- `assert_healthy()` fails on decode errors, kicks, and anomalies a vanilla
  client tolerates but that point at a server bug: an entity added twice, a
  block update for a chunk the client does not hold, a forget for a chunk it
  never received.
- `hold_teleports(true)` stops answering position syncs, so a scenario can
  play a lagging client and send `ServerboundAcceptTeleportation` itself.
- `disconnect()` closes the connection like a player quitting.

### Waiting Without Sleeping

Never sleep to let the server catch up. Wait for something observable:

- `expect` and `wait_until` wait for what the client receives.
- `wait_for` waits for what the server holds.
- `bot.sync()` returns once the server has handled every packet the bot sent
  before the call, finished that tick, and every packet it sent the bot during
  that tick has arrived. Use it before asserting that something did **not**
  happen.

Every wait times out after 15 seconds with a message naming what it waited
for. `sync()` sends a `Pong` with the high bit set, which the harness answers
with a `Ping` carrying the same id at the end of the tick.

### Maintenance Rules

- **Protocol bumps wait for Azalea.** The suite fails fast when Azalea and
  `voidmc-protocol` speak different protocol versions. A pull request that
  bumps `PROTOCOL_VERSION` waits until Azalea publishes a release for that
  version, then bumps the `azalea-*` crates in `void-e2e/Cargo.toml` with it.
- **The nightly moves with Azalea.** Change `void-e2e/rust-toolchain.toml` only
  when bumping Azalea, to a nightly that release builds on.
- **Flaky scenarios are quarantined, not retried.** CI does not retry failed
  tests. A scenario that fails intermittently gets `#[ignore = "flaky: <issue>"]`
  pointing at an issue, and is fixed from there.
- **No sleeps.** Wait for an observable condition, or `sync()` before a
  negative assertion.

### Functional and Load Runs

The crate also ships `voidmc-load`, which runs a named workload's smoke checks
and full-protocol load clients against an in-process server or an
operator-supplied endpoint, and writes a report with a PASS or FAIL decision.
The `smoke@1` workload runs as part of this suite. See
[Load Testing](/performance/load-testing).

### What the Scenarios Cover

Each regression below was injected into the server, one at a time, and made at
least one scenario fail.

| Regression | Caught by |
|---|---|
| Chunk section header off by two bytes | every scenario |
| Compression announced but not enabled | every scenario with compression |
| Chunks no longer forgotten behind a moving player | `chunks_follow_the_player_and_are_forgotten_behind_it` |
| Block change broadcast to the wrong chunk | `block_changes_reach_other_players_and_late_joiners` |
| Block change broadcast but not stored | `block_changes_reach_other_players_and_late_joiners` |
| Keep-alive answers never accepted | `keep_alive_round_trips_and_keeps_going` |
| A player that never reports loaded is never timed out | `a_player_that_never_reports_loaded_and_stops_answering_is_timed_out` |
| A connection stalled before play lingers | `a_client_stalled_in_login_is_told_it_took_too_long`, `a_client_stalled_in_configuration_is_told_it_took_too_long`, `a_connection_that_never_logs_in_is_closed` |
| Wrong serverbound packet id for container clicks | `clicks_move_items_between_slots` |
| Suggestion ranges in bytes instead of UTF-16 units | `server_suggestions_use_utf16_ranges` |
| Joining player not announced to others | `players_see_each_other_move_and_leave` |
| Quitting player not removed for others | `players_see_each_other_move_and_leave` |
| Streamed chunks generated at swapped coordinates | `optimized_generation_matches_the_fixture_at_spawn_and_across_a_chunk_boundary` |
| Revisited chunks regenerated instead of read from `ChunkIndex` | `optimized_generation_matches_the_fixture_at_spawn_and_across_a_chunk_boundary` |
| `max_chunk_generations_per_tick` ignored | `optimized_generation_matches_the_fixture_at_spawn_and_across_a_chunk_boundary` |
| Spawn area not generated at startup | `optimized_generation_matches_the_fixture_at_spawn_and_across_a_chunk_boundary` |
| Spawn Y one block above the surface | every `atp_027_world_generation` scenario |
| Terrain generated from chunk-local coordinates | `optimized_and_reference_paths_generate_identical_terrain` |
| One more dirt layer in `DefaultWorldGenerator` | `optimized_and_reference_paths_generate_identical_terrain` |
| Join radius larger than the spawn area leaves holes | `the_whole_view_arrives_when_the_spawn_area_is_smaller_than_the_join_radius` |
| Client Information not stored | `client_settings_are_stored_from_configuration_and_from_play` |
| Player chat sent to clients on "Commands Only" or "Hidden" | `players_who_hide_chat_are_sent_no_player_chat_but_still_command_feedback`, `a_chat_setting_changed_while_playing_applies_to_the_next_line` |
| Player chat sent to ready players only | `a_player_still_loading_the_world_is_sent_chat` |

## Testing Best Practices

### What to Test

✅ **Must test:**

- Edge cases (0, negative, overflow, max value)
- Error conditions
- State transitions
- Critical paths

✅ **Should test:**

- Typical use cases
- Integration points
- Performance-critical code

### What NOT to Overly Test

❌ **Don't test:**

- Third-party library behavior (they have their own tests)
- Trivial getter functions
- The compiler (trust Rust's type system)

### Coverage Goals

Aim for coverage on:

- **Critical code**: 95%+ (codec, protocol parsing)
- **Core logic**: 80%+ (game state, handlers)
- **Utilities**: 70%+ (helpers, formatting)

```bash
# Generate coverage report (the CI tool)
cargo llvm-cov --workspace --all-features --html
```

## Debugging Tests

### Getting More Information

```bash
// Enable backtrace
RUST_BACKTRACE=1 cargo test

// Show println! output
cargo test -- --nocapture

// Run single-threaded for deterministic output
cargo test -- --test-threads=1
```

### Using println! in Tests

```rust
#[test]
fn test_with_debug_output() {
    let result = some_function();
    println!("Result: {:?}", result);  // Shows when test fails
}
```

### Interactive Debugging

```bash
# Run test in debugger
rust-gdb --args cargo test test_name -- --test-threads=1
```

## Continuous Integration

All tests run automatically on:

- Every commit push
- Every pull request
- Before merge to main

Tests must pass before code can be merged. The `e2e` job runs the
[end-to-end suite](#end-to-end-tests) on its pinned nightly; every other job
uses the stable toolchain in `rust-toolchain.toml`.

## Common Test Patterns

### Testing Result Types

```rust
#[test]
fn valid_input_returns_ok() {
    let result = parse("valid");
    assert!(result.is_ok());
}

#[test]
fn invalid_input_returns_err() {
    let result = parse("invalid");
    assert!(result.is_err());
}

#[test]
fn error_message_is_descriptive() {
    let result = parse("invalid").unwrap_err();
    assert!(result.contains("expected"));
}
```

### Testing Panic Behavior

```rust
#[test]
#[should_panic(expected = "divide by zero")]
fn divide_by_zero_panics() {
    divide(10, 0);
}
```

### Testing with Fixtures

```rust
fn setup() -> TestEnvironment {
    TestEnvironment::new()
}

#[test]
fn test_with_setup() {
    let env = setup();
    // Use env...
}
```

## Test Maintenance

- Keep tests synchronized with code changes
- Remove obsolete tests
- Refactor duplicate test code
- Update test names if behavior changes
