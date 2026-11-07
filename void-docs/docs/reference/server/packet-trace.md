# Packet Trace

The trace writes one JSON line per packet, in and out, with its tick and the system that
produced it. It is a development tool: it is off by default, and a build without the feature
contains none of it.

## Turning it on

Build with the `packet-trace` feature of `voidmc`, and add the layer to your subscriber, so
that lines can name their system:

```rust
use tracing_subscriber::prelude::*;

tracing_subscriber::registry()
    .with(tracing_subscriber::fmt::layer())
    .with(voidmc::packet_trace::trace_layer())
    .init();
```

Set up the subscriber before `VoidServer::run`: a system opens its span when it is added.

Filter per layer, as above or with `fmt::layer().with_filter(filter)`. A global filter, such as
`tracing_subscriber::fmt().with_env_filter(..)`, also drops the spans the trace reads, and lines
then have no `system`. The server warns when the layer is missing, but not in this case:
`voidc trace` shows it as an attribution under 100 %.

| Variable | Effect |
|---|---|
| `VOID_TRACE=<file>` | Writes the trace there. Unset, the feature does nothing but keep `bevy_ecs` spans. |

## The format

```json
{"tick":661,"dir":"out","client":1,"player":"sonde","packet":"Play/UpdateEntityPosition","bytes":9,"hash":123,"system":"voidmc::systems::entities::broadcast_entity_movement"}
```

| Field | Meaning |
|---|---|
| `tick` | `voidmc::pace::of(world).ticks_started()`: the number of ticks started, 1 during the first. |
| `dir` | `out` or `in`. |
| `client` | The connection id. It changes with whatever connected first. |
| `player` | The name given at login. Absent before it. |
| `packet` | The state and the packet, or `Play/?` for a packet that did not decode. |
| `bytes` | The packet id and its payload: no frame length, and before any compression. |
| `hash` | FNV-1a 64 of those bytes. It exceeds what `jq` keeps exactly. |
| `system` | The innermost system running at that moment. Absent when there is none. |
| `dropped` | Why an outgoing packet was lost: `full`, `kicked` or `closed`. Absent when it was queued. |

The file is flushed at the end of every tick, and when a thread panics. If a write fails, on a
full disk for instance, the trace stops there and logs one error: the last line may be cut, and
`voidc trace` skips it.

## What it does not show

- A line without `dropped` means the packet reached the client's queue, not the client.
- The network thread serves status connections by itself: the handshake, the status request and
  the ping of a server list query are not in the trace.
- An incoming packet is not listed when it reaches a client that is already gone, because it is
  dropped before it is decoded.
- `bytes` is not the traffic on the wire: add the frame length, and expect less once packets
  are compressed.
- A command deferred from an observer runs when the triggering system's world flushes it, so its
  packets are attributed to that system and not to the observer.
- Each outgoing packet is encoded a second time while the trace runs, because the hash needs
  the bytes where the system is known. The network thread encodes its own copy.

## Reading it

`voidc trace` filters a trace, totals it by system, packet and client, and compares two runs:
see `void-clients/README.md`.

## Reproducible UUIDs

A stepped server draws the UUIDs of entities and boss bars from a fixed seed and runs its
systems one at a time, so two stepped runs that receive the same packets at the same ticks send
the same UUIDs, with no feature.

On a normal server they are random.
