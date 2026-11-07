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
"contains". `state` lists the fields. Conditions, clocks and the socket in detail:
[reference](../void-docs/docs/guide/voidc.md#verbs).

Bots add `packets`.

`logic` and `render` add `turn dx dy`, `aim yaw pitch`, `seq ms dx,dy ...`, `attack`, `use`,
`shift on|off`, `key code action`, `hover x y`, `click x y`, `free`, `type text`, `esc`, `fps n`,
`dump`, `track n`, `frames n`. A `track` or `frames` is refused while the last one still records.

`render` adds `shot name [downscale]`, `thumb`, `show [focus]`, `hide`.

Frame by frame: `clock virtual|real`, `step <ms> [n]`, `fence <n>`, `sent`. `voidc step` drives them.

`state` also gives `loading`, the camera as `cam_x`, `cam_y`, `cam_z`, `cam_yaw`, `cam_pitch`, and
`clock`, `virtual` or `real`. `voidc cmd` is a shortcut for `~/.voidc/run/<name>.sock`, which takes
one verb per line and answers one JSON line.

## Packet traces

A server built with the `packet-trace` feature writes every packet to `VOID_TRACE`, with its
tick and its system. The format is in `void-docs/docs/reference/server/packet-trace.md`.

```sh
VOID_TRACE=a.jsonl cargo run -p voidmc-example --features packet-trace
voidc trace a.jsonl                              # totals by system, packet and client
voidc trace a.jsonl --system stream_chunks --from 600 --to 700 --json
voidc trace a.jsonl --player hot-reload --packet SpawnEntity --lines
voidc trace diff a.jsonl b.jsonl                 # exits with 1 when the runs differ
```

Filters are `--client N`, `--player NAME`, `--packet TEXT`, `--system TEXT`, `--dir in|out`,
`--from TICK` and `--to TICK`. `--json` prints one JSON object, and `--lines` the lines kept. An
unknown option or a flag without its value is an error.

`diff` compares outgoing packets tick by tick and player by player; how it lines two runs up is in
the [reference](../void-docs/docs/guide/voidc.md#packet-traces).

## Frame by frame

`voidc step` plays a passage on a locked server, so that an animation can be judged without
watching it. Each run needs a fresh server, started with a step socket:

```sh
PORTS=$(voidc port "Ouverture")
PORT=${PORTS% *}
SOCK=~/.voidc/run/ouverture.step
VOID_EXAMPLE_STEP_SOCKET=$SOCK VOID_EXAMPLE_ADDRESS=127.0.0.1:$PORT cargo run -p voidmc-example &
voidc step "Ouverture" --server $SOCK --out shots --ticks 120 --frames 4 --stop-server
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

A failure stops the run with exit code 1, and `--stop-server` stops the server then as well.
`--hold T:S` pauses S seconds after tick T. One client per server, and `logic` takes `--every 0`.
What repeats from run to run: [reference](../void-docs/docs/guide/voidc.md#frame-by-frame).

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
  [Frame by frame](#frame-by-frame). Every run on `logic` or `render` ends with `clock real`.

A scenario that cannot run is refused before its first verb. A run stops at its first failed line
unless `--keep-going`, and exits with 1. `--keep-up` leaves the client running, and `--out`
chooses the folder of `report.txt`, `report.json`, the shots, the differences and the recordings.
The rules: [reference](../void-docs/docs/guide/voidc.md#scenarios).

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

## Checking an animation

```sh
voidc check frames.txt cut=21 images=pictures --json out.json
```

It reads the JSONL of a stepped run, `{"t":tick,"k":image,"x":..,"y":..,"z":..,"yaw":..,"pitch":..}`,
and the `frames.txt` and `track.txt` of the verbs, whose ticks come from their clock.

| Error | When | Option |
|---|---|---|
| `jump past 64 blocks` | more than 64 blocks a tick: what a client's glide survives | none |
| `camera jump` | more than 24 blocks in a tick. The fastest real flight of `/map` makes 6 | `jump=` |
| `half turn` | 120 degrees or more between two frames, the short way round | `turn=` |
| `flash` | the brightness moves by 30 levels of 255 between two pictures | `flash=` |

`cut=56`, `cut=56:2` and `cut=56-60` declare what is meant to happen there. More in the
[reference](../void-docs/docs/guide/voidc.md#checking-an-animation).

## The cost of a frame

```sh
voidc perf carte                    # all of it: F3+L, GPU, JFR
voidc perf carte ab clouds          # with and without clouds, in turns
voidc perf carte ab particles --count 16000
```

`metrics` is the F3+L recording by section, `gpu` whole frames with `TimerQuery`, `jfr` packets by
type, and `ab` the only GPU figure to trust. Options: `--seconds`, `--frames`, `--rounds`,
`--count`. The report lands in `~/.voidc/clients/<name>/perf/`. What spoils a figure:
[reference](../void-docs/docs/guide/voidc.md#the-cost-of-a-frame).

## What it guards

Limits on clients, ports and versions, and what a run promises: see the
[reference](../void-docs/docs/guide/voidc.md#what-it-guards). Limits and levels live in
`clients.toml`, built into the binary. Runtime state lives in `~/.voidc`, `VOIDC_HOME` moves it.

## The mosaic

`voidc up` starts it, `voidc dash` prints its address. It shows every client with its state,
starts and stops them, and hands one over: "Prendre la main" shows the window and gives it the
focus, "Rendre la main" hides it again.

## How the window stays hidden

`probe/HideAgent.java` hides the window before GLFW opens it, and `logic` runs on the LWJGL agent of
HeadlessMC: see the [reference](../void-docs/docs/guide/voidc.md#how-the-window-stays-hidden).
