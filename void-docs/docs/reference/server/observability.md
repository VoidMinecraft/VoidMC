# Health and Metrics Endpoints

VoidMC can serve a health check and Prometheus metrics over HTTP. The feature is
**off by default**: nothing listens unless the application calls
`ServerConfigBuilder::observability`.

```rust
use std::time::Duration;
use voidmc::{ObservabilityConfig, ServerConfigBuilder};

let config = ServerConfigBuilder::new()
    // 127.0.0.1:9464; ObservabilityConfig::new("10.0.0.5:9464") picks another interface.
    .observability(ObservabilityConfig::localhost(9464))
    .build();

// Optional: report a stall sooner or later than the default.
let config = ServerConfigBuilder::new()
    .observability(ObservabilityConfig::localhost(9464).stall_threshold(Duration::from_secs(5)))
    .build();
```

The endpoints have **no authentication and no TLS**. Bind them to the loopback
interface (`127.0.0.1`, the documented default host,
`DEFAULT_OBSERVABILITY_HOST`) or to a private network that only your monitoring
can reach, never to a public interface.

`VoidServer::run` binds the address before the game loop starts and panics if it
cannot. Port `0` asks the operating system for a free port; the address actually
bound is available to systems as the `ObservabilityAddress` resource, the same
way `ListenAddress` reports the game port.

The example server enables it with an environment variable: a bare port binds
`127.0.0.1`, a `host:port` binds that address.

```bash
VOID_OBSERVABILITY_ADDR=9464 cargo run -p voidmc-example
curl -i http://127.0.0.1:9464/health
curl -s http://127.0.0.1:9464/metrics
```

## How it Runs

The HTTP side runs on a thread of its own with a single-threaded Tokio runtime,
never on the game thread, so it still answers while a tick is stuck. The game
thread and the network thread record into shared atomic counters; a request only
reads them.

The responder is a deliberately small HTTP/1.1 server, with no dependency beyond
Tokio:

- one request per connection, answered with `Connection: close`;
- `GET` and `HEAD` on `/health` and `/metrics`; the query string is ignored;
- unknown paths get `404`, other methods on a known path `405` with
  `Allow: GET, HEAD`, a malformed request line `400`, an HTTP version other than
  1.0 or 1.1 `505`;
- a request head over 8 KiB gets `431`, a client that has not sent its whole
  head after 5 seconds gets `408`;
- at most 32 connections are served at once; more are closed on accept.

A slow or broken client only holds its own connection, never the game loop.

## `/health`

| State | HTTP status | Meaning |
|---|---|---|
| `starting` | `503` | No tick has completed yet. |
| `ok` | `200` | A tick completed within the stall threshold. |
| `stalled` | `503` | No tick has completed for longer than the stall threshold. |
| `stopping` | `503` | The server is shutting down. |

The stall threshold defaults to 40 tick intervals (2 seconds at 20 TPS) and is
never less than one second; `ObservabilityConfig::stall_threshold` overrides it.

The body is a small JSON object, served as `application/json`:

```json
{"status":"ok","ticks":1234,"last_tick_age_ms":12,"stall_threshold_ms":2000}
```

`last_tick_age_ms` is `null` before the first tick.

### What an Operator Sees When the Server Stops

Shutdown begins when the game loop receives `AppExit` (`/stop`, Ctrl-C or
SIGTERM, or an application writing the message). From the end of that tick,
`/health` answers `503` with `"status":"stopping"`, while the world is torn down.
The listener is closed before `VoidServer::run` returns; from then on the port
refuses connections. A monitor should treat `503` and a refused connection alike:
the server is not serving players.

## `/metrics`

Served as `text/plain; version=0.0.4; charset=utf-8`, the Prometheus text
exposition format. Every family has `# HELP` and `# TYPE` lines, counters end in
`_total` and never decrease while the process runs.

Only values the server measures are exposed. A gauge that has no value yet has
no sample rather than a made-up one.

### Tick

| Metric | Type | Meaning |
|---|---|---|
| `voidmc_ticks_total` | counter | Game ticks completed. |
| `voidmc_tick_duration_seconds` | histogram | Time the game thread spent on one tick, from the start of `First` to the end of `Last` (buckets 1 ms to 2.5 s). Excludes the sleep between ticks. |
| `voidmc_target_ticks_per_second` | gauge | Configured tick rate. |
| `voidmc_ticks_per_second` | gauge | Ticks per second measured over the last full one-second window. No sample until the first window ends. |
| `voidmc_seconds_since_last_tick` | gauge | Time since the last tick completed, computed when scraped. No sample before the first tick. |

### Players and Connections

| Metric | Type | Meaning |
|---|---|---|
| `voidmc_players_online` | gauge | Players in the play state at the end of the last tick; the count the server list shows. |
| `voidmc_players_max` | gauge | Configured maximum player count. |
| `voidmc_players_joined_total` | counter | Players that reached the play state. |
| `voidmc_connections_accepted_total` | counter | TCP connections accepted on the game port, server-list pings included. |

### Errors

| Metric | Type | Meaning |
|---|---|---|
| `voidmc_connection_errors_total{kind}` | counter | Game connections closed by an error. `kind="io"`: socket error, including a client that vanished with a reset; `kind="frame"`: invalid framing or compression; `kind="decode"`: a packet the network thread could not decode. A client closing its connection normally is not an error. |
| `voidmc_clients_kicked_total` | counter | Live connections the server closed itself: `/kick` and `Players::kick`, logins refused because the server is full, keep-alive timeouts (30 s) of players in the play state whether ready or not, connections that did not reach the play state within the pre-play timeout (30 s), clients that sent a malformed packet, outbound queues that overflowed, and client entities despawned by game code. Players still online at shutdown are not counted. |
| `voidmc_packets_unrecognized_total` | counter | Packets dropped because their id is unknown in the connection's state (logged as `Unrecognized packet`); the client stays connected. |
| `voidmc_packet_errors_total` | counter | Packets with a known id the game thread failed to decode; each disconnects its sender, which also counts as a kick (logged at INFO as `Client sent a malformed packet; disconnecting`). See [undecodable packets](../protocol/networking.md#undecodable-packets). |
| `voidmc_chunk_load_errors_total{kind}` | counter | Stored chunks that could not be loaded and were generated instead, one per `ChunkLoadFailedEvent`. `kind="decode"`: the stored bytes were read but could not be decoded; `kind="io"`: they could not be read. Only a chunk loader such as `voidmc-world-io` produces them; see [Damaged chunks](/reference/server/world-serialization#damaged-chunks). |

Player gauges are updated at the end of each tick, so while the game loop is
stalled they keep their last value; `voidmc_seconds_since_last_tick` shows how
old it is.

## Relation to `metrics_debug`

`metrics_debug` and `metrics_tps_output` are a separate, older diagnostic: they
log a TPS sample every second and write it to a CSV file. They do not open any
port, and the endpoints above do not need them.
