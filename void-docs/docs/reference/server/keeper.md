# Development Proxy

`voidmc-keeper` sits between a vanilla client and a Void server. When the server
restarts or crashes, the client stays connected and in game, and picks up the new
server when it comes back. It is a development tool: offline mode, one server, no
encryption.

`void-example` takes the default port, 25565, so the client joins the proxy on 25566:

```sh
VOIDMC_STABLE_PLAYER_IDS=1 cargo run -p voidmc-example &
cargo run -p voidmc-keeper -- --backend 127.0.0.1:25565 --listen 127.0.0.1:25566
```

| Option | Default | Meaning |
|---|---|---|
| `--backend ADDRESS` | required | The server, as `[::1]:25671` |
| `--listen ADDRESS` | `127.0.0.1:25565` | Where clients connect |
| `--return-timeout SECONDS` | `120` | How long a client waits for a lost server |
| `--settle SECONDS` | `3` | How long a new server has before old chunks and packs go |
| `--backend-silence SECONDS` | `20` | How long a server may stay silent before it counts as lost, from 15 to 25: a quiet server only sends a keep-alive every 200 ticks, 10 seconds at 20 TPS, and a client hangs up after 30 seconds without a packet |

## The two paths

While the server is away the client stays in play, its world on screen, with
`Server away for Ns, hold on` in the action bar. The proxy sends it keep-alives and
tries to log in every 100 ms. The decision is taken as late as possible, while the new
server configures:

- **Seamless.** The proxy logs in for the client and follows the frames of the last
  configuration, compared by fingerprint. If the new server does the same thing, the
  client never sees a configuration: the proxy swallows the `login`, undoes what the
  client still shows, and the relay resumes. No screen appears.
- **Reconfigured.** At the first difference, the client is told `start_configuration`
  and receives what the server has said so far. This is a real configuration, with a
  short loading screen, and every doubt about a client in play falls back to it.

Either way the player starts over where the new server puts it: at spawn, with a fresh
inventory. The client keeps its connection, not its game.

A client lost in the middle of a configuration has neither path: it adds up the
registries it receives and fails on a double when the configuration ends, and it cannot
start over. The proxy keeps from it what the new server repeats, and goes on live from
there. A server that says anything else ends the session with a message, and so does a
server that asks what only the client can answer: a cookie, a code of conduct, a dialog
or a transfer. `resumed` then
comes with path `reconfigured` and reason `the server went away during a configuration`.
Resource packs stay out of this comparison: a push of a pack the client does not hold yet
reaches it at once. A client that receives a push for an id it already has replaces that
pack without a word to the server, then answers for the new one from the start. If the
download of the replaced pack then fails, the client fails the new pack of that id with
it, and the server receives `DOWNLOAD_FAILED` for a pack that had no fault of its own.
The one exception is a client that already received `finish_configuration`: it no longer reads
configuration packets, so a pack the proxy would have to pass on ends the session with a
message.

A client that received `login_finished` and has not acknowledged it yet waits for that
acknowledgement before the proxy logs in again, so that a new server never tells it twice.
Once it acknowledges, it waits the whole `--return-timeout`, not the 20 seconds of a login.

A client that ended its configuration but never received its `login` holds nothing to
compare: it goes straight through the reconfigured path, with reason
`the client never received a login`.

The seamless path needs all of the following; `resumed.reason` names the one that failed:

| Condition | Otherwise |
|---|---|
| The configuration is frame for frame the same, registries and tags included | `the configuration changed at packet N (id 0x..)` |
| The player entity id is the same | `player id changed from A to B; start the server with VOIDMC_STABLE_PLAYER_IDS=1 ...` |
| The `login` describes the same level (dimension, seed, sea level, hardcore and the like) | `the level changed: ... became ...`, with both descriptions |
| The ledger knows everything the client holds | The packet that broke it |

## The ledger

Whatever a server sends as a whole replaces itself and needs no tracking: commands,
tags, recipes, chunks at the same position. What adds up is held by id only, never by
content, and undone before the new server speaks. Every clientbound play packet is
classified, and a test walks all the ids of `voidmc_data` so that a new packet cannot
go unclassified:

| Case | Examples | At the return |
|---|---|---|
| Leaves no state, or state every server sends again | movement, sounds, particles, time, chunk center | Nothing |
| Leaves state, undone | entities, boss bars, objectives, teams, tab entries, titles, effects, attributes, cooldowns, inventory, cursor, open menu, dialog, camera, vehicle, passengers, game mode, abilities, border, rain, distances | Removed or reset to what a fresh `login` gives |
| Leaves state it cannot undo | `block_destruction`, `waypoint`, `open_book`, `open_sign_editor`, `player_combat_kill`, the game events for the credits, the respawn screen and limited crafting | Reconfigured path |
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
id must not change. With `VOIDMC_STABLE_PLAYER_IDS` set to anything but empty, `0` or
`false` (or
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
| `resumed` | `session`, `path`, `reason`, `outage_ms`, tally, `ledger_bytes` | The new server took the client over; tally and bytes are what the client held before |
| `gave_up` | `session`, `reason` | `--return-timeout` ran out, the client was disconnected with this reason |
| `client_left` | `session` | |

`resumed` is the signal that a server is really back for a client. `backend_up` only
says that the status answers, and comes before a login is possible. `resumed` itself
leaves as soon as the `login` is taken over or the client ends its configuration: the
chunks of the new server are still on their way, so the world is not yet on screen.
`outage_ms` and the times below measure up to that point.

Events go through a bounded queue on a thread of their own: a reader that stalls or dies
loses events and slows nothing. Lost events are counted, and said on stderr at the first
one, then at every power of two, with the total when the proxy stops. On Ctrl-C the
proxy writes what is queued, for at most 2 seconds, before it exits.

## Refusals and failures

- **Compression** is handled on the server side only: the client link stays plain. A
  client reads at most 2 097 151 bytes on a plain link, so a packet above that, once
  inflated, ends the session with a message giving its size.
- **Online mode** and a **different protocol version** are refused with a clear message.
- A client that leaves while the server is away ends its session, and the others go on.
- A disconnect, and anything else the proxy writes itself, follows what the client
  decodes, not what it has answered: `login_finished`, `start_configuration` and
  `finish_configuration` switch a client as they arrive. Between `finish_configuration`
  and the client's answer the proxy sends no keep-alive. A disconnect that falls inside
  an open bundle closes it first: the client holds a bundle's packets until it ends.
- A frame cut by the crash never reaches the client. A packet that cannot be read is
  still relayed, and forces the reconfigured path at the next return.
- A server silent for `--backend-silence` counts as lost, in play and during a return,
  and is tried again. A write stuck for 15 seconds does the same. The default stays
  below the 30 seconds after which a client hangs up, and above the 10 seconds between
  two keep-alives of the engine, which it counts in ticks: 200 of them, 10 seconds at 20 TPS,
  more on a server that lags. A value under 15 or over 25 seconds is refused. A server paused
  in a debugger counts as lost too.
- A difference only the `login` shows, such as another player id, costs one more login:
  the new server sees the player join, leave, and join again, with the client
  reconfigured the second time. Plugins that react to a join run twice.

## Measures

These measures come from `voidmc-map`, a server that is not on `main`, built with a `fast`
profile, the server killed with `SIGKILL` and restarted at once. `resumed`
runs from the kill to the event; in game runs to the player standing at spawn, camera on
the player, no screen; chunks runs to the 289 chunks around the spawn.

| Case | Client | `resumed` | In game | Chunks |
|---|---|---|---|---|
| Seamless, 3 runs | `voidc` | 0.77 s to 0.82 s | 0.78 s to 0.83 s | 0.93 s to 0.95 s |
| Seamless, 5 runs | `voidc` bot | 0.71 s to 0.90 s | 0.75 s to 0.95 s | |
| Reconfigured, player id changed | `voidc` | 0.91 s to 0.93 s | 1.12 s to 1.16 s | 1.23 s to 1.42 s |
| Reconfigured, `voidmc-map` replaced by `voidmc-checkers` | `voidc` bot | 0.23 s to 0.25 s | | |
| Server killed during a configuration, first join | `voidc` | 0.76 s to 1.12 s | | |

Most of that time is the server starting. The proxy itself used about 7.5 MB of resident
memory and 0.45 s of CPU for a whole series of these runs, and the ledger held
17 KB of ids. Per packet, on a loaded machine (`cargo bench -p voidmc-keeper`):

| Operation | Time |
|---|---|
| Packet that leaves no state | 6 ns |
| New entity | 14 ns |
| Chunk | 27 to 47 ns |
| Teardown of 1000 entities and 1024 chunks | 18 to 35 µs |

## Limits

- Sounds already playing and map item data stay.
- Commands typed before the restart are not replayed.
- `/kick` and other disconnects from a live server pass through to the client and end
  its session there, without `backend_lost`.
