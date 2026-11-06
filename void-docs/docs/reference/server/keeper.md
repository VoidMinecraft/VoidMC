# Development Proxy

`voidmc-keeper` sits between a vanilla client and a Void server. When the server
restarts or crashes, the client stays connected and in game, and picks up the new
server when it comes back. It is a development tool: offline mode, one server, no
encryption.

```sh
VOIDMC_STABLE_PLAYER_IDS=1 VOID_MAP_ADDRESS=127.0.0.1:25566 cargo run --profile fast -p voidmc-map &
cargo run --profile fast -p voidmc-keeper -- --backend 127.0.0.1:25566 --listen 127.0.0.1:25565
```

| Option | Default | Meaning |
|---|---|---|
| `--backend ADDRESS` | required | The server, as `[::1]:25671` |
| `--listen ADDRESS` | `127.0.0.1:25565` | Where clients connect |
| `--return-timeout SECONDS` | `120` | How long a client waits for a lost server |
| `--settle SECONDS` | `3` | How long a new server has before old chunks and packs go |

## The two paths

While the server is away the client stays in play, its world on screen, with
`Server away for Ns, hold on` in the action bar. The proxy answers its keep-alives
and tries to log in every 100 ms. The decision is taken as late as possible, while
the new server configures:

- **Seamless.** The proxy logs in for the client and follows the frames of the last
  configuration, compared by fingerprint. If the new server does the same thing, the
  client never sees a configuration: the proxy swallows the `login`, undoes what the
  client still shows, and the relay resumes. No screen appears.
- **Reconfigured.** At the first difference, the client is told `start_configuration`
  and receives what the server has said so far. This is a real configuration, with a
  short loading screen, and it is always possible, so every doubt falls back to it.

The seamless path needs all of the following; `resumed.reason` names the one that failed:

| Condition | Otherwise |
|---|---|
| The configuration is frame for frame the same, registries and tags included | `the configuration changed at packet N (id 0x..)` |
| The player entity id is the same | `player id changed from A to B; start the server with VOIDMC_STABLE_PLAYER_IDS=1 ...` |
| The `login` describes the same level (dimension, seed, sea level, hardcore and the like) | The field that differs |
| The ledger knows everything the client holds | The packet that broke it |

## The ledger

Whatever a server sends as a whole replaces itself and needs no tracking: commands,
tags, recipes, chunks at the same position. What adds up is held by id only, never by
content, and undone before the new server speaks. Every clientbound play packet is
classified, and a test walks all the ids of `voidmc_data` so that a new packet cannot
go unclassified:

| Case | Examples | At the return |
|---|---|---|
| Leaves no state | movement, sounds, particles, time | Nothing |
| Leaves state, undone | entities, boss bars, objectives, teams, tab entries, titles, effects, attributes, cooldowns, inventory, cursor, open menu, dialog, camera, vehicle, passengers, game mode, abilities, border, rain, distances | Removed or reset to what a fresh `login` gives |
| Leaves state it cannot undo | `block_destruction`, `waypoint` | Reconfigured path |
| Unknown or unreadable | | Reconfigured path |

Order matters, and each of these comes from the client's own code:

- The camera goes back to the player before entities go: `set_camera` is ignored for
  an entity the client no longer has.
- The player leaves its vehicle before the vehicle goes.
- A bundle cut by the crash is closed first.
- **The player's own tab entry is never removed.** `AbstractClientPlayer` caches the
  `PlayerInfo` it first reads and takes its game mode from it. Once removed, the player
  keeps reading the dead entry whatever the server says; if that entry said spectator,
  the player falls through the ground. The proxy updates its game mode instead, and
  the client merges the entry the new server adds into the existing one.

Old chunks and server packs stay until `--settle` runs out, then whatever the new
server did not send again goes. A pack the client already holds is not pushed again:
the proxy answers the server what the client would have answered.

## Player ids

A client cannot change its entity id without a new level: `handleLogin` on an existing
player keeps the old level, and `handleRespawn` always shows the loading screen. So the
id must not change. With `VOIDMC_STABLE_PLAYER_IDS` set (or
`ServerConfigBuilder::stable_player_ids`), the engine derives the id from the UUID, in
a range the allocator does not reach, and falls back to the allocator on a collision.

Rewriting ids in the proxy was rejected: ids hide in entity metadata, passengers,
links and more, and one missed field, or a server entity that lands on the rewritten
id, removes the local player. Without the variable nothing breaks: the proxy takes the
reconfigured path and says which variable would avoid it.

## Events

Stdout carries one JSON object per line, for tools; logs go to stderr.

| Event | Fields | Meaning |
|---|---|---|
| `listening` | `address`, `backend` | The proxy accepts clients |
| `backend_up`, `backend_down` | `backend` | Status probe, every 500 ms |
| `client_joined` | `session`, `name` | |
| `backend_lost` | `session`, tally | The server went away under a client |
| `resumed` | `session`, `path`, `reason`, `outage_ms`, tally, `ledger_bytes` | The client is back |
| `gave_up` | `session`, `reason` | `--return-timeout` ran out, the client was disconnected with this reason |
| `client_left` | `session` | |

`resumed` is the signal that a server is really back for a client. `backend_up` only
says that the status answers, and comes before a login is possible.

## Refusals and failures

- **Compression** is handled on the server side only: the client link stays plain.
- **Online mode** and a **different protocol version** are refused with a clear message.
- A client that leaves while the server is away ends its session, and the others go on.
- A frame cut by the crash never reaches the client. A packet that cannot be read is
  still relayed, and forces the reconfigured path at the next return.

## Measures

`voidmc-map`, profile `fast`, `/map` open, 70 entities and 814 chunks held, server
killed with `SIGKILL` and restarted at once. Time runs from the kill to `resumed`.

| Case | Client | Time |
|---|---|---|
| Seamless | Vanilla 26.1.2 | 0.73 s, 1.19 s |
| Seamless | `voidc` bot | 0.72 s to 1.44 s |
| Reconfigured, id changed | Vanilla 26.1.2 | 1.00 s |
| Reconfigured, `voidmc-map` replaced by `voidmc-checkers` | `voidc` bot | 0.41 s |

Most of that time is the server starting. The proxy itself used about 4 MB of resident
memory and 0.27 s of CPU for a whole run with a vanilla client, and the ledger held
17 KB of ids. Per packet, on a loaded machine (`cargo bench -p voidmc-keeper`):

| Operation | Time |
|---|---|
| Packet that leaves no state | 6 ns |
| New entity | 14 ns |
| Chunk | 27 to 47 ns |
| Teardown of 1000 entities and 1024 chunks | 18 to 35 µs |

## Limits

- A server that hangs without closing its socket is not detected; the client times out
  by itself.
- Sounds already playing and map item data stay.
- Commands typed before the restart are not replayed.
- `/kick` and other disconnects from a live server pass through to the client.
