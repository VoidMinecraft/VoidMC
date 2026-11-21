# Acceptance-test tools

Tools for the VoidMC acceptance test plan (ATP, `atp_679_VoidMC.xlsx`). Each objective
in the plan names the tool it needs and the output to compare. They are test
instrumentation: none of them is part of the product, and none is a vanilla client.

All scripts need Python 3.10 or later and the standard library only. Run them from the
repository root. Every script prints its usage with `--help`.

## Scripts

| Script | Objectives | What it does |
|---|---|---|
| `status-ping.py` | 002, 024 | Server-list status and ping: version, protocol, MOTD, player counts. |
| `capture-proxy.py` | 019, 024 | Transparent TCP forwarder (default `127.0.0.1:25566` → `127.0.0.1:25565`) that records each connection to `OUT/conn-N-*.bin`. |
| `chunk-order.py` | 019, 024 | Replays a recording: view-square shape, unloads, duplicates, re-sends and nearest-first order, including the join. |
| `inventory-clicks.py` | 014 | Offline protocol client that sends invalid, stale, forged and malformed container clicks and checks the server-side item totals. |
| `health-poll.py` | 031 | Polls `/health` every 5 ms and prints each state change, so short states such as `stopping` can be seen. |
| `metrics-check.py` | 031 | Scrapes `/metrics` and checks the Prometheus format and the documented metric families. |
| `region-trace-analyze.py` | 028 | Compares a SingleThreaded and a MultiThreaded run of `patches/region-parallel.patch`. |
| `atplib.py` | – | Shared protocol helpers (varints, stream decoding, a minimal offline client). |

## Fixture patches

The patches change the example server (and, for `region-parallel.patch`, one framework
file) for one test. They are made against the commit under test. Apply one at a time,
and remove it after the test:

```bash
git apply tools/atp/patches/<name>.patch
# ... run the objective ...
git apply -R tools/atp/patches/<name>.patch
```

The first lines of each patch describe what it does.

| Patch | Objectives | Change |
|---|---|---|
| `inventory-trace.patch` | 014 | Logs `ATP inventory` with the server's own slots after every change. |
| `physics-arena.patch` | 015, 018 | Builds a walled stone arena at Y 110 in chunk (1,1). |
| `remove-hello-command.patch` | 021 | Removes the example's `/hello` registration. |
| `event-observers.patch` | 023, 033, 034 | A plugin with a resource, `/atpprobe` and observers that log `ATP event ...`. |
| `config-values.patch` | 024 | Sets sample `ServerConfigBuilder` values (MOTD, max players, tick rate, radii, view distance). |
| `shutdown-flush-only.patch` | 026 | Sets the periodic save interval to 9,999,999 ticks. |
| `striped-generator.patch` | 027, 035 | A custom `WorldGenerator`: surface at Y 80, gold and grass stripes along X. |
| `region-parallel.patch` | 028 | 120 deterministic pigs in two regions, per-tick state and per-region timing traces. |
| `stall-command.patch` | 031 | `/atpstall` blocks the game thread for 6 seconds. |

When the example's `main.rs` changes, check that the patches still apply
(`git apply --check`) and build (`cargo check -p voidmc-example`), and regenerate them
if needed.
