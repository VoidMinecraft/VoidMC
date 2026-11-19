# Load Testing

`voidmc-load` runs a defined Minecraft workload against a VoidMC server with
full-protocol clients and decides PASS or FAIL against that workload's
thresholds. It lives in the end-to-end crate, `void-e2e/`, and its clients are
the same Azalea-based `Bot`s the [end-to-end scenarios](/guide/testing#end-to-end-tests)
use: each one performs the handshake, login, configuration and play states,
answers keep-alives, pings and teleports like the vanilla client, and decodes
every packet the server sends.

The TCP connect stress POC in `pocs/performance/` measures listener accepts
only; `voidmc-load` measures what players do.

> **Scope.** A report applies only to the workload, thresholds, host and run
> conditions it records. It is not a capacity claim: it does not say how many
> players VoidMC supports, nor how it behaves under another workload or on
> another host. Every report carries this statement.

## What a Run Does

1. **Smoke checks.** One client walks the functional path a player takes, and
   each step is a named check: `status_ping` (the server-list ping answers
   with protocol 775), `join` (login, configuration, spawn), `chunks_loaded`
   (the chunk under its feet arrives), `chat_echo` (its chat message is
   broadcast back), `command_reply` (`/ping` answers), `chunk_boundary_crossing`
   (walking 16 blocks east re-centres its chunk view) and `healthy` (no
   protocol anomaly). The checks stop at the first failure; the rest are
   reported as skipped, and the load phase is skipped too.
2. **Load.** The workload's clients join one after another over the ramp-up
   window, then act until the end of the steady window. Each client runs one
   cycle per action interval: it walks a step along its own compass
   direction, out to the roam radius and back, so the clients spread over
   several chunks; it sends a chat message and waits for its broadcast; it
   runs `/ping` and waits for the reply.
3. **Decision.** Each measurement is compared with its threshold, and the run
   passes only if none fails.

## Workloads

A workload is fixed and versioned (`name@version`): changing a client count, a
duration, an action or a threshold makes a new version. The definitions are in
`void-e2e/src/load/workload.rs`.

| | `smoke@1` | `standard@1` |
|---|---:|---:|
| Load clients | 4 | 20 |
| Ramp-up | 1 s | 5 s |
| Steady window | 2 s | 30 s |
| Action interval | 500 ms | 1 s |
| Step / roam radius | 4 / 32 blocks | 4 / 32 blocks |
| Command | `/ping` | `/ping` |
| Nominal tick rate | 20 TPS | 20 TPS |
| `min_connection_success` | 100 % | 100 % |
| `min_tick_rate` | 10 TPS | 18 TPS |
| `max_latency_p95_ms` | 2000 ms | 250 ms |
| `max_protocol_errors` | 0 | 0 |
| `max_unexpected_disconnects` | 0 | 0 |
| `max_timeouts` | 0 | 0 |
| `max_other_errors` | 0 | 0 |
| `max_server_log_failures` | 0 | 0 |
| `max_failed_smoke_checks` | 0 | 0 |

`smoke@1` runs in the end-to-end suite on every change, on shared CI runners
and busy developer machines. Its tick-rate and latency limits catch a broken or
stalled server without depending on how fast the host is. `standard@1` is the
opt-in profile for a dedicated host or a private staging server: it asks the
server to keep within 10 % of its nominal tick rate and to answer within about
five ticks.

## Measurements

| Measurement | How it is obtained | In-process | Operator endpoint |
|---|---|---|---|
| Connection success | Load clients that complete login, configuration and spawn, over clients started | yes | yes |
| Latency | Chat and command round trips seen by the clients (p50, p95, max), including the wait for the next tick | yes | yes |
| Tick rate, server | Game-time advance per second over the steady window, read on the server | yes | unavailable |
| Tick rate, clients | Game-time advance per second between two world-time updates each client receives | yes | only if the server runs a world clock |
| Errors | Undecodable packets and protocol anomalies, kicks and closed connections, timeouts (15 s), others; the first 20 messages are kept | yes | yes |
| Server log failures | Errors and unrecognised packets VoidMC logged during the run | yes | unavailable |
| Resources | CPU time of the game thread and of every server thread (game thread, Bevy task pools, network runtime), process CPU time and peak resident memory over the steady window, from Linux procfs | yes | unavailable |

The in-process server shares its process with the load clients, so the
process-wide CPU and memory figures include both; the per-thread figures cover
the server alone. The in-process server is the end-to-end harness's (view and
simulation distance 4) started at the nominal tick rate, with a running
overworld clock so that clients receive world-time updates as they would from a
vanilla world.

`min_tick_rate` is judged on the server's own count when the server runs
in-process, otherwise on the slowest client's. Against an endpoint that sends
no world-time update, and for the server log of an endpoint, the threshold is
reported as not applicable and the report lists the measurement under
`unavailable`; everything else that was not observed fails its threshold.

## Running It

The tool runs from `void-e2e/` on the end-to-end suite's nightly toolchain (unset
`RUSTUP_TOOLCHAIN` if it is set). It prints a Markdown summary, writes the JSON
report and the summary next to it with `--report`, and exits with 0 on PASS,
1 on FAIL and 2 on a usage error.

```bash
cd void-e2e

# Smoke-sized run against a server started in this process on 127.0.0.1 and
# an ephemeral port.
cargo run --bin voidmc-load -- --workload smoke --report reports/smoke.json

# Full-sized run, in-process.
cargo run --bin voidmc-load -- --workload standard --report reports/standard.json

# Against a server you started, such as a private staging server. There is no
# default address: the endpoint target refuses to run without one.
cargo run --bin voidmc-load -- --target endpoint --endpoint staging-host:25565 \
  --workload standard --report reports/staging.json
VOIDMC_LOAD_ENDPOINT=staging-host:25565 cargo run --bin voidmc-load -- --target endpoint
```

`cargo run` uses the dev profile, whose dependencies the end-to-end crate
compiles with optimisations; add `--release` for a release build. The report
records which profile ran. Point the tool only at servers you operate: it logs
in players named `smoke-<run>` and `load-<run>-<n>`, in offline mode. Reports
are generated output; do not commit them.

Set `VOID_E2E_LOG` to an `EnvFilter` directive, such as `voidmc_e2e=info`, to
see progress and, in-process, the server's logs.

## The Report

The JSON report (`report_format` `voidmc.load.report.v1`) has these sections:

| Key | Content |
|---|---|
| `workload` | `id`, every parameter and threshold, and the actions in words |
| `conditions` | start time (UTC), wall-clock duration, commit and whether the tree had uncommitted changes, build profile, CPU cores available to the run (`host_cores`), OS and architecture, endpoint kind (`in-process` or `operator-endpoint`) and address, in-process server settings |
| `measurements` | smoke checks with their outcome and detail, whether the load ran, connections, latency, tick rate, errors, resources |
| `unavailable` | each measurement the run could not make, and why |
| `verdicts` | each threshold with its rule, limit, observed value and outcome (`pass`, `fail`, `not_applicable`) |
| `decision`, `failed_thresholds` | `PASS` or `FAIL`, and the thresholds that failed |
| `scope` | the statement that the results are not a capacity claim |

## In the Test Suite

`void-e2e/tests/scenarios/atp_031_load.rs` runs `smoke@1` in-process and
against an endpoint, and checks the report's content and its PASS decision. It
also induces each kind of failure in a real server, a client kicked on join, a
client kicked mid-run, an error in the server log, a server below the tick-rate
threshold, an unreachable endpoint, and checks that the run fails and names the
violated threshold. Unit tests in `void-e2e/src/load/` drive the decision with
recorded measurements, one violated threshold at a time. The heavier
`standard@1` profile never runs in CI.
