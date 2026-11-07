# Development Proxy

`voidmc-keeper` sits between a vanilla client and a Void server. When the server
restarts, crashes or stops, the client stays connected, and goes back in game when the
server returns. It is a development tool: offline mode, one server, no encryption.

`void-example` takes the default port, 25565, so the client joins the proxy on 25566:

```sh
cargo run -p voidmc-example &
cargo run -p voidmc-keeper -- --backend 127.0.0.1:25565 --listen 127.0.0.1:25566
```

| Option | Default | Meaning |
|---|---|---|
| `--backend ADDRESS` | required | The server, as `[::1]:25671` |
| `--listen ADDRESS` | `127.0.0.1:25565` | Where clients connect |
| `--return-timeout SECONDS` | `120` | How long a client waits for a lost server |
| `--settle SECONDS` | `3` | How long a new server has to offer its packs before the others go |
| `--backend-silence SECONDS` | `20` | How long a server may stay silent before it counts as lost, from 15 to 25: a quiet server only sends a keep-alive every 200 ticks, 10 seconds at 20 TPS, and a client hangs up after 30 seconds without a packet |

## The return

While the server is away the client stays in play, its world on screen, with
`Server away for Ns, hold on` in the action bar. The proxy sends it keep-alives and
tries to connect every 100 ms. A clean stop and a crash look the same: the engine closes
its connections without a disconnect packet, and the proxy counts the server as lost.

As soon as a new server accepts the connection, the client is told
`start_configuration`. The proxy logs in on its behalf, holds what the server says until
the client acknowledges, then relays it: the client goes through a real configuration,
with a short loading screen. The client resets by itself what the old server left on
it: entities, boss bars, titles, scoreboard, tab list, camera, dialog, sounds, chunks.
The proxy clears its own message from the action bar right after the new `login`.

The player starts over where the new server puts it: at spawn, with a fresh inventory.
The client keeps its connection, its chat and its resource packs, not its game.

A client lost in the middle of a configuration cannot be told to start over: it adds up
the registries it receives and fails on a double when the configuration ends. The new
server must repeat what the client already holds, which the proxy keeps from it, and the
relay goes on live from there. A server that says anything else ends the session with a
message, and so does a server that asks what only the client can answer: a cookie, a code
of conduct, a dialog or a transfer. Resource packs stay out of this comparison: a push of
a pack the client does not hold yet reaches it at once. A client that receives a push for
an id it already has replaces that pack without a word to the server, then answers for
the new one from the start. If the download of the replaced pack then fails, the client
fails the new pack of that id with it, and the server receives `DOWNLOAD_FAILED` for a
pack that had no fault of its own. The one exception is a client that already received
`finish_configuration`: it no longer reads configuration packets, so a pack the proxy
would have to pass on ends the session with a message.

A client that received `login_finished` and has not acknowledged it yet waits for that
acknowledgement before the proxy logs in again, so that a new server never tells it twice.
Once it acknowledges, it waits the whole `--return-timeout`, not the 20 seconds of a login.

## Resource packs

A pack the client already holds is not pushed again: the proxy answers the server what
the client answered the first time, and the client never reloads its resources. Packs
the new server did not offer again go once `--settle` runs out.

## Events

Stdout carries one JSON object per line, for tools; logs go to stderr.

| Event | Fields | Meaning |
|---|---|---|
| `listening` | `address`, `backend` | The proxy accepts clients |
| `backend_up`, `backend_down` | `backend` | Status probe, every 500 ms |
| `client_joined` | `session`, `name` | |
| `backend_lost` | `session` | The server went away under a client |
| `resumed` | `session`, `outage_ms` | The client ended the configuration of the new server |
| `gave_up` | `session`, `reason` | `--return-timeout` ran out, the client was disconnected with this reason |
| `client_left` | `session` | |

`resumed` is the signal that a server is really back for a client. `backend_up` only
says that the status answers, and comes before a login is possible. `resumed` leaves as
soon as the client ends its configuration: the `login` and the chunks of the new server
are still on their way, so the world is not yet on screen.

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
  an open bundle closes it first, and so does a crash: the client holds a bundle's
  packets until it ends.
- A frame cut by the crash never reaches the client.
- A server silent for `--backend-silence` counts as lost, in play and during a return,
  even once the client acknowledged `start_configuration`, and is tried again. A client
  still downloading a pack holds the server back, so that wait does not count. A write stuck for 15 seconds does the same. The default stays
  below the 30 seconds after which a client hangs up, and above the 10 seconds between
  two keep-alives of the engine, which it counts in ticks: 200 of them, 10 seconds at 20 TPS,
  more on a server that lags. A value under 15 or over 25 seconds is refused. A server paused
  in a debugger counts as lost too.

## Measures

Each case ran 3 times with a `voidc` client, release builds of `voidmc-example` and of the
proxy, and `voidmc-map` for packs. Times run from the signal to the client in game: at
spawn, no screen, its chat echoing a message sent after the signal. The client never
reconnected.

| Case | In game | Seen by the client |
|---|---|---|
| `SIGKILL` in play, `void-example` | 0.53 s to 0.58 s | A configuration screen for about 0.3 s |
| Clean stop (`SIGINT`, the server exits in under 0.1 s) | 0.54 s to 0.63 s | The same |
| `SIGKILL` during the return, before the client ended its configuration | 0.53 s to 0.58 s | The same |
| `SIGKILL` during the first join, then a restart | 0.53 s to 0.68 s | Its first configuration goes on |
| `SIGSTOP` for 18 s, then `SIGCONT`, with `--backend-silence 15` | 0.40 s to 0.46 s | A configuration screen during the freeze |
| `SIGKILL` in play, `voidmc-map` and its pack | 0.93 s to 0.99 s | No pack reload: the proxy answers for the pack |

Most of that time is the server starting.

## Limits

- Commands typed before the restart are not replayed.
- `/kick` and other disconnects from a live server pass through to the client and end
  its session there, without `backend_lost`.
