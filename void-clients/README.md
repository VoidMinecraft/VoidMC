# void-clients

`voidc` is the one tool for the Minecraft clients of the agents. Install it once, it works
from every worktree:

```sh
cargo install --path void-clients
```

## Testing a feature

```sh
voidc port "Hot reload"              # 25700 25701, the same pair every time
VOID_DEMO_ADDRESS=127.0.0.1:25700 cargo run -p voidmc-demo &
voidc up "Hot reload"                # returns once the client is in game
voidc cmd hot-reload wait_until 15000 "chunks>=100"
voidc cmd hot-reload shot before     # prints the path of the PNG
voidc cmd hot-reload assert "chunks>=100"
voidc down hot-reload
```

The map serves its resource pack on the second port of the pair, and `act map` opens it:

```sh
VOID_MAP_ADDRESS=127.0.0.1:25700 VOID_MAP_PACK_ADDRESS=127.0.0.1:25701 cargo run -p voidmc-map &
voidc up "Hot reload"
voidc cmd hot-reload act map
voidc cmd hot-reload wait_until 15000 "passenger==true"
```

A failed `assert`, `wait_until` or verb exits with 1. A feature is one or two words: the
window title is `SONDE` and the feature. The client's name is the feature in lower case.

## Levels

Take the lightest level that can see what you test.

| Level | Costs | Sees | Does not see |
|---|---|---|---|
| `bot` | 3 MB, joins in 0.3 s | Packets, entities, chunks, camera, chat | Everything the client computes or draws |
| `logic` | 0.8 GB, no GPU | The real client state: screens, riding, aiming, entities | Any picture |
| `render` (default) | 1.6 GB | Everything drawn: captures, shaders, packs, interpolation | Real focus, real mouse, sound |

`voidc play "Feature" --port N` starts a visible client for Léo. It takes no command.

Measuring frames? Start with `--fps 120`, or send `fps 120`. Add `--jfr` for a recording.

## Verbs

All levels: `state`, `act <command>`, `wait_until [ms] <condition>`, `assert <condition>`,
`entities`, `quit`. A condition is `field op value`, with `>=`, `<=`, `==`, `!=`, and `~` for
"contains". `state` lists the fields. A field starts with a letter and a value is one word: a value
with spaces goes between quotes, and the limit of a wait comes first.
`wait_until 5000 'action_bar~"Tour 2"'` waits 5 s for "Tour 2"; `wait_until state!=connecting 5000`
is an error, never a wait. Only spaces and tabs separate words: another space or a control
character, or an invisible format character like U+200B, in a condition is an error.
`conditions.tsv` holds the cases, with `\t`, `\\` and `\uXXXX` escapes: `cargo test`, which needs `javac`, runs them through the Rust and Java code that splits a
verb and reads a condition, the code the bot and the probe use. An error case may add a fifth cell, a
piece of the message both sides must give, never empty. `>=` and `<=` need a number, written `-?\d+(\.\d+)?`.

Bots add `packets`.

`logic` and `render` add `turn dx dy`, `aim yaw pitch`, `seq ms dx,dy ...`, `attack`, `use`,
`shift on|off`, `key code action`, `hover x y`, `click x y`, `free`, `type text`, `esc`, `fps n`,
`dump`, `track n`, `frames n`. A `track` or `frames` is refused while the last one still records.
Their lines carry the time of the game, so the probe refuses a `clock virtual` while one
records, and names the recording: wait for its end. A scenario refuses it before its first verb.
`clock real` is never refused: a recording that stamps a line after the time went back ends with
an `invalid:` line, and the run refuses it and says why. One that stamps nothing more, such as a
`track` whose camera stopped, stays valid.

`render` adds `shot name [downscale]`, `thumb`, `show [focus]`, `hide`.

Frame by frame: `clock virtual|real`, `step <ms> [n]`, `fence <n>`, `sent`. `voidc step` drives them.

`state` also gives `loading`, true while the game shows its loading screen, the camera as
`cam_x`, `cam_y`, `cam_z`, `cam_yaw`, `cam_pitch`, and `clock`, `virtual` or `real`. A virtual
clock outlives a run cut short by Ctrl-C: the client stays frozen, with no read timeout and no
reconnection. A run that finds it so stops before its first verb, unless the scenario starts with
`clock real`; `voidc cmd <name> clock real` gives it back. A client that does not tell its clock
has a probe older than `voidc`: its run stops the same way, and is told to restart the client. A
`clock` line that such a probe does not know fails with the same advice, and the run then sends
no `clock real` at its end. A `shot` of a scenario waits for the loading
screen to go; `voidc cmd` takes the picture at once.

`voidc cmd` is a shortcut for the socket: `~/.voidc/run/<name>.sock` takes one verb per line
and answers one JSON line, with none of the checks of `voidc`. A condition reads the same everywhere, in `void-clients/src/condition.rs`
and in `probe/Condition.java`, as [Verbs](#verbs) says: `wait_until 30000 chunks >= 200` waits 30 s
and `wait_until chunks >= 200 30000` is refused. `voidc cmd` refuses what a scenario refuses, before sending it:
`>=` or `<=` on a text field such as `chat`, against a value that is not a number (`1e3`, `5.` and
`200 30s` are not), or a condition with no value, such as `screen==`.

On every level, `>=` and `<=` fail with an error when the value read or the value given is not a
number, instead of answering.

## Packet traces

A server built with the `packet-trace` feature writes every packet to `VOID_TRACE`, with its
tick and its system. The format is in `void-docs/docs/reference/server/packet-trace.md`.

```sh
VOID_TRACE=a.jsonl cargo run -p voidmc-example --features packet-trace
VOID_TRACE=b.jsonl cargo run -p voidmc-map --features packet-trace     # the map has the feature too
voidc trace a.jsonl                              # totals by system, packet and client
voidc trace a.jsonl --system stream_chunks --from 600 --to 700 --json
voidc trace a.jsonl --player hot-reload --packet SpawnEntity --lines
voidc trace diff a.jsonl b.jsonl                 # exits with 1 when the runs differ
```

Filters are `--client N`, `--player NAME`, `--packet TEXT`, `--system TEXT`, `--dir in|out`,
`--from TICK` and `--to TICK`. `--json` prints one JSON object, and `--lines` the lines kept. An
unknown option or a flag without its value is an error.

Totals and `bytes` cover what was queued. Packets lost on the way, with a `dropped` field, are
apart: `out_dropped`, `dropped_bytes`, `dropped_by_system` and `dropped_by_packet`.

`diff` compares outgoing packets, tick by tick and player by player, on their name and their
hash: pass `--dir in` for the other way. It tells ticks that hold the same packets in another
order from ticks that differ, and gives the systems of each side at the first difference. A
tick present in one run only counts as a difference, and so does a packet lost in one run and
not in the other, or lost for another reason.

Two runs only line up tick for tick on a stepped server, and only from the tick it locks at:
pass `--from` with that tick. Otherwise `--align PACKET` makes the first such packet tick 0 in
each trace, which compares the shape of a run, not its content.

## Frame by frame

`voidc step` plays a passage on a locked server, so that an animation can be judged without
watching it. Each run needs a fresh server, started with a step socket:

```sh
P=($(voidc port "Ouverture"))
SOCK=~/.voidc/run/ouverture.step
VOID_EXAMPLE_STEP_SOCKET=$SOCK VOID_EXAMPLE_ADDRESS=127.0.0.1:${P[1]} cargo run -p voidmc-example &
voidc step "Ouverture" --server $SOCK --out shots --ticks 120 --frames 4 --stop-server
```

The map reads `VOID_MAP_STEP_SOCKET`, serves its pack on the second port, and `--act map` opens it
before the first tick:

```sh
VOID_MAP_STEP_SOCKET=$SOCK VOID_MAP_ADDRESS=127.0.0.1:${P[1]} VOID_MAP_PACK_ADDRESS=127.0.0.1:${P[2]} cargo run -p voidmc-map &
voidc step "Ouverture" --server $SOCK --out shots --ticks 120 --frames 4 --act map --stop-server
```

A run first deletes what an earlier run wrote in `--out`. It starts the client, runs the server at
game speed until tick `--lock-at` (600), fails if the client is not in game with `--min-chunks`
(200) chunks by then, and freezes both. It then plays `--ticks` ticks of `--frames`
frames, and stops the client. `--act` is a command sent just before the first tick; repeat it to send several, in order.

| Written to `--out` | |
|---|---|
| `frames.jsonl` | One line per frame: `{"t":tick,"k":frame_in_tick,"x":..,"y":..,"z":..,"yaw":..,"pitch":..}`, the camera as drawn |
| `fNNNNN.png` | The pictures asked for: `--every N` (1 by default, 0 for none) and `--at i,j`, counted in frames |
| `video.mp4` | Game speed with `--every`, 4 pictures per second with `--at`, from `ffmpeg` |
| `sheet.png` | At most 24 pictures, evenly picked |

A client cut from the server, a barrier that expires or a client that does not answer stops the
run with an error and exit code 1. `--stop-server` stops the server after a failure as well. A
server refuses a socket that another server answers on. `--hold T:S` pauses S seconds after tick T.

The camera repeated exactly in every run measured, at both levels. An assertion on `frames.jsonl`
can start with no tolerance, but it holds on one condition: every deadline of the server goes
through `Pace`, which never lets one pass on a stepped server. A server with deadlines of its own in
real time, to send chunks for instance, lets a loaded machine shift a chunk by a tick, and with it
what depends on that chunk.

Pictures do not repeat exactly, mostly because animated textures start at a different phase. With
frozen textures the median difference drops from 428 to 17 pixels. Compare pictures by zone.

One client per server, because the counters of the barrier cover all clients. `logic` takes
`--every 0`.

## Scenarios

`voidc run` plays a file, one verb a line, with no sleep and no script around it.

```text
# scenarios/open-map.scn
level render
wait_until 30000 chunks >= 100
frames 500
act map
wait_until 15000 passenger==true
wait_still 800 15000
shot open
compare open refs/map.png zones=refs/map.zones
check frames cut=15-30
```

```sh
voidc run open-map.scn --feature "Carte" --port 25700   # starts a client, stops it afterwards
voidc run open-map.scn --on carte --keep-going          # on a running client, plays every line
```

The verbs of the level are there, except `show`, `hide`, `thumb`, `perf` and `quit`, which a
scenario refuses as unknown. It adds:

- `level bot|logic|render`: its level. `--level` overrides it, and a client given with `--on`
  must have it.
- `wait_still <ms> [limit]`: until the camera has not moved for that long. Take a picture after
  it, or it catches a flight halfway.
- `compare <a> <b> ...`: a shot of this scenario, or a file next to it.
- `check frames|track|<file> ...`: `frames` and `track` run while the lines after them play.
- `clock`, `step`, `fence`, `sent`: the stepping verbs of the probe, see
  [Frame by frame](#frame-by-frame). `clock` takes `virtual` or `real`. At its end, once its
  recordings are in, every run on `logic` or `render` sends `clock real`, whatever it played: the
  probe does nothing when the clock is already real. If the client refuses it or cannot be
  reached, the run fails and its report says so. A recording is read whole once it ends: one the
  probe marked `invalid:` fails the run, checked or not, and leaves nothing in `--out`, not even
  the file of an earlier run.
  A bot has no clock and gets nothing.

A verb the level does not have, a word missing, a picture that does not exist, a `check frames`
with no `frames` above it, a second `frames` or `track` before the `check` of the first, a `clock virtual`
between a `frames` or `track` and its `check`, `>=` on a text or a field no client has stops the run before its first
verb. A condition is sent as written, so it reads as in `voidc cmd`; a `wait_until` starts with its limit in ms. `compare` and `check` read what this run made and the files of the scenario, never
a file left in the folder by an earlier run. A recording with fewer lines than asked fails. A run stops at its first failed line unless
`--keep-going`, and exits with 1. `--keep-up` leaves the client running, `--out` chooses the
folder, by default `~/.voidc/clients/<name>/runs/`. It holds `report.txt`, `report.json` with the
line and the value received for each failure, the shots, the differences and the recordings.

## Comparing pictures

```sh
voidc compare a.png b.png zones=map.zones --diff out.png
```

| Option | Meaning |
|---|---|
| `max=N` | at most N pixels differ in the whole picture |
| `tile=N` | at most N pixels differ in any 32x32 tile |
| `delta=N` | a pixel differs from this gap on a channel, 16 by default, and `require=` takes it |
| `ignore=x,y,w,h` | this zone counts nowhere |
| `require=x,y,w,h:max:delta` | at most `max` pixels differ here, 0 by default, at its own `delta` |
| `zones=file` | these options, one a line, `#` for a comment. Options beside it override it |

A comparison without a limit is refused. The picture of the difference shows what changed in red
over the dimmed first picture, ignored zones in blue and required zones in green.

Two runs of `/map` differ by 10,000 to 35,000 pixels: actors move, and a flipped square of 64
pixels adds 1,900. No total catches it. So compare a few clean runs first, ignore what moves, and
require the rest, where two clean runs differ by nothing at all.

## Checking an animation

```sh
voidc check frames.txt cut=21 images=pictures --json out.json
```

It reads the JSONL of a stepped run, `{"t":tick,"k":image,"x":..,"y":..,"z":..,"yaw":..,"pitch":..}`,
and the `frames.txt` and `track.txt` of the verbs, whose ticks come from their clock.

| Error | When | Option |
|---|---|---|
| `jump past 64 blocks` | more than 64 blocks a tick: what a client's glide survives, see `void-map/src/players.rs` | none |
| `camera jump` | more than 24 blocks in a tick. The fastest real flight of `/map` makes 6 | `jump=` |
| `half turn` | 120 degrees or more between two frames, the short way round | `turn=` |
| `flash` | the brightness moves by 30 levels of 255 between two pictures | `flash=` |

A jump past 64 blocks across missing ticks, within the limit by time, is a `WARN` line of the
report and fails nothing, and so is a `frames.txt` or `track.txt` whose lines all carry the same
time: recorded under a virtual clock that nothing stepped, it can show no movement. A recording
that ends with an `invalid:` line, written by the probe when a line was stamped after `clock real`
gave the time back, is an error that names this cause. `images=` holds one PNG for each frame, or `<tick>-<image>.png` or `f<index>.png` for some of them; `sheet.png`
is left out. `cut=56`, `cut=56:2`
and `cut=56-60` declare what is meant to happen: there it is no error, and a cut where nothing
happens is one. Without a locked clock a tick moves from one run to the next, so declare a range.

## The cost of a frame

```sh
voidc perf carte                    # all of it: F3+L, GPU, JFR
voidc perf carte ab clouds          # with and without clouds, in turns
voidc perf carte ab particles --count 16000
```

- `metrics`: the F3+L recording by section, ten seconds at most. CPU submission, not GPU time.
- `gpu`: whole frames with `TimerQuery`. Entities are stopped on the client and the frame limiter
  is off during the measure: with it, a sample is only the length of its wait.
- `jfr`: packets by type, from a recording started while in game.
- `ab`: the only GPU figure to trust. It alternates two settings without rebuilding a chunk, gives
  the difference with its 95 % interval from at least 6 rounds, and says so when a round has no
  sample or chunks were rebuilt, which exits with 1, when the two sides draw the same picture, or
  when nothing stands clear of the noise. `particles` makes its own,
  as a map has none: the figure is the price of that many in front of the camera.

Take it on a calm machine: the report counts the other clients and the entities that still moved,
which packets of the server can do. Options: `--seconds`, `--frames`, `--rounds`, `--count`. The
report lands in `~/.voidc/clients/<name>/perf/`.

## What it guards


- At most `max_clients` clients. Bots do not count.
- A port leased to a feature is refused to another one, and a lease skips `[reserved]` and
  anything listening.
- `voidc down` only signals a pid that is still that client, so it cannot kill a neighbour.
- A client to play has no socket, so it cannot be driven, and there is only one.
- A client goes back to its server by itself after a restart.
- A client or a bot with no verb for `idle_minutes` stops, unless its window is shown. `state` and
  `thumb` do not count as verbs. `VOIDC_IDLE_MINUTES` changes that for one launch.
- A `voidc up` that fails stops the client it started, even when its record cannot be written.
- A `wait_until` sent to a real client still loading waits for the game within its limit.
- Each `VOIDC_HOME` has its own mosaic: `~/.voidc` on the port of `clients.toml`, any other home on
  a port leased in `~/.voidc/ports.json`, so two homes, or a home and a feature of `~/.voidc`, never
  share one. `voidc up` replaces a mosaic of its own home and another version, fails if it
  does not quit, never touches the mosaic of another home, and warns about one from before versions.
- A condition that cannot be compared, like `chunks>=abc`, is an error, never true.
- The mosaic acts only for its own page: a request from another site or another host name is refused.
- A `VOIDC_HOME` too long for a socket is refused at once.
- `voidc cmd`, `run`, `step` and `perf` refuse a client started by another version of the probe or
  the bot, and say to restart it: every verb of `voidc` passes through one check. The socket itself
  checks nothing, so a verb written straight to it reaches any client. `list`, `down` and the
  mosaic still see, stop, show and hide such a client. The version is a hash of their sources outside tests (the probe, the bot,
  conditions, framing, launch), not of the protocol crates. A client tells it in every answer, from
  the moment its socket opens, even while it loads.
- A `voidc run` or `perf` opens one connection and checks the version once. A verb that fails takes
  that connection down and the next verb opens and checks another, so a late answer never passes
  for the next verb. A game stuck for more than 10 s still answers `state`, with a failure; one
  silent for 12 s is said not to answer.
- The bot renders translated chat from the game's `en_us.json`, read from the jar with `unzip`.
- `voidc up` and `voidc step` warn when the disk has less than `min_free_gib` free, and still
  start.

Limits and levels live in `clients.toml`, built into the binary. Runtime state lives in
`~/.voidc`, `VOIDC_HOME` moves it.

## The mosaic

`voidc up` starts it, `voidc dash` prints its address. It shows every client with its state,
starts and stops them, and hands one over: "Prendre la main" shows the window and gives it the
focus, "Rendre la main" hides it again.

## How the window stays hidden

`probe/HideAgent.java` adds `GLFW_VISIBLE=0` at the end of `GlBackend.setWindowHints`, because
`GLX._initGlfw` resets any hint set earlier, and `GLFW_COCOA_MENUBAR=0` before `glfwInit`, which
keeps the process in the background. The game still renders into its own target, so captures
work. `-XstartOnFirstThread` stays, and macOS needs no Xvfb.

`logic` uses the LWJGL agent of [HeadlessMC](https://github.com/headlesshq/headlessmc), downloaded
once and checked against its SHA-256. `probe/Probe.java` is compiled against the game jar on first
launch, so a renamed method fails at compile time.
