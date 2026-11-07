# Test clients: voidc

`voidc` starts Minecraft clients that open no window, drives them, and checks what they see. The
commands and their examples are in `void-clients/README.md`. This page holds the rules behind them.

## Verbs

A condition is `field op value`, with `>=`, `<=`, `==`, `!=`, and `~` for "contains". A field starts with a letter and a value is one word: a value
with spaces goes between quotes, and the limit of a wait comes first.
`wait_until 5000 'action_bar~"Tour 2"'` waits 5 s for "Tour 2"; `wait_until state!=connecting 5000`
is an error, never a wait. Only spaces and tabs separate words: another space or a control
character, or an invisible format character like U+200B, in a condition is an error.
`conditions.tsv` holds the cases, with `\t`, `\\` and `\uXXXX` escapes: `cargo test`, which needs `javac`, runs them through the Rust and Java code that splits a
verb and reads a condition, the code the bot and the probe use. An error case may add a fifth cell, a
piece of the message both sides must give, never empty. `>=` and `<=` need a number, written `-?\d+(\.\d+)?`.

A `track` or `frames` is refused while the last one still records. Their lines carry the time of the game, so the probe refuses a `clock virtual` while one
records, and names the recording: wait for its end. A scenario refuses it before its first verb.
`clock real` is never refused: a recording that stamps a line after the time went back ends with
an `invalid:` line, and the run refuses it and says why. One that stamps nothing more, such as a
`track` whose camera stopped, stays valid.

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

A client cut from the server, a barrier that expires or a client that does not answer stops the
run with an error and exit code 1. `--stop-server` stops the server after a failure as well, on a
new connection when the failure lost the old one. A server refuses a socket that another server
answers on. `--hold T:S` pauses S seconds after tick T.

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

`clock`, `step`, `fence` and `sent` are the stepping verbs of the probe. `clock` takes `virtual` or
`real`. At its end, once its
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

Two runs of `/map` differ by 10,000 to 35,000 pixels: actors move, and a flipped square of 64
pixels adds 1,900. No total catches it. So compare a few clean runs first, ignore what moves, and
require the rest, where two clean runs differ by nothing at all.

## Checking an animation

A jump past 64 blocks across missing ticks, within the limit by time, is a `WARN` line of the
report and fails nothing, and so is a `frames.txt` or `track.txt` whose lines all carry the same
time: recorded under a virtual clock that nothing stepped, it can show no movement. A recording
that ends with an `invalid:` line, written by the probe when a line was stamped after `clock real`
gave the time back, is an error that names this cause. `images=` holds one PNG for each frame, or `<tick>-<image>.png` or `f<index>.png` for some of them; `sheet.png`
is left out. `cut=56`, `cut=56:2`
and `cut=56-60` declare what is meant to happen: there it is no error, and a cut where nothing
happens is one. Without a locked clock a tick moves from one run to the next, so declare a range.

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
- `voidc cmd`, `run` and `step` refuse a client started by another version of the probe or
  the bot, and say to restart it: every verb of `voidc` passes through one check. The socket itself
  checks nothing, so a verb written straight to it reaches any client. `list`, `down` and the
  mosaic still see, stop, show and hide such a client, and the end of a run still gives it its
  clock back. The version is a hash of their sources outside tests (the probe, the bot,
  conditions, framing, launch), not of the protocol crates. A client tells it in every answer, from
  the moment its socket opens, even while it loads.
- A `voidc run` opens one connection and checks the version once. A verb that fails takes
  that connection down and the next verb opens and checks another, so a late answer never passes
  for the next verb. A game stuck for more than 10 s still answers `state`, with a failure. Opening
  gives `state` 12 s, and a client silent that long is said not to answer. Once open, every verb has
  30 s, `state` included, and a `wait_until` its limit and 5 s.
- The `clock real` at the end of a run takes a connection of its own, with no check and 30 s, so
  that a connection down or a probe of another version does not stop it. A probe that does not
  know it fails the run, in the report. One case still ends virtual: a `clock virtual` that timed
  out on a stuck game can leave its `freeze` queued in the probe, to land after that `clock real`.
  The preflight of the next run sees it and refuses, unless that run starts with `clock real`.
- The bot renders translated chat from the game's `en_us.json`, read from the jar with `unzip`.
- `voidc up` and `voidc step` warn when the disk has less than `min_free_gib` free, and still
  start.

Limits and levels live in `clients.toml`, built into the binary. Runtime state lives in
`~/.voidc`, `VOIDC_HOME` moves it.

## How the window stays hidden

`probe/HideAgent.java` adds `GLFW_VISIBLE=0` at the end of `GlBackend.setWindowHints`, because
`GLX._initGlfw` resets any hint set earlier, and `GLFW_COCOA_MENUBAR=0` before `glfwInit`, which
keeps the process in the background. The game still renders into its own target, so captures
work. `-XstartOnFirstThread` stays, and macOS needs no Xvfb.

`logic` uses the LWJGL agent of [HeadlessMC](https://github.com/headlesshq/headlessmc), downloaded
once and checked against its SHA-256. `probe/Probe.java` is compiled against the game jar on first
launch, so a renamed method fails at compile time.
