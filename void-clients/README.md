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
"contains". `state` lists the fields. A field starts with a letter and a value is one word: a value
with spaces goes between quotes, and the limit of a wait comes first.
`wait_until 5000 'action_bar~"Tour 2"'` waits 5 s for "Tour 2"; `wait_until state!=connecting 5000`
is an error, never a wait. Only spaces and tabs separate words: another space or a control
character, or an invisible format character like U+200B, in a condition is an error.
`conditions.tsv` holds the cases, with `\t`, `\\` and `\uXXXX` escapes: `cargo test`, which needs `javac`, runs them through the Rust and Java code that splits a
verb and reads a condition, the code the bot and the probe use. `>=` and `<=` need a number, written `-?\d+(\.\d+)?`.

Bots add `packets`.

`logic` and `render` add `turn dx dy`, `aim yaw pitch`, `seq ms dx,dy ...`, `attack`, `use`,
`shift on|off`, `key code action`, `hover x y`, `click x y`, `free`, `type text`, `esc`, `fps n`,
`dump`, `track n`, `frames n`.

`render` adds `shot name [downscale]`, `thumb`, `show [focus]`, `hide`.

`voidc cmd` is a shortcut for the socket: `~/.voidc/run/<name>.sock` takes one verb per line
and answers one JSON line.

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
- `voidc up` replaces a mosaic of another version, fails if it does not quit, and warns about one
  from before versions.
- A condition that cannot be compared, like `chunks>=abc`, is an error, never true.
- The mosaic acts only for its own page: a request from another site or another host name is refused.
- A `VOIDC_HOME` too long for a socket is refused at once.
- `voidc cmd` refuses a client started by another version of the probe or the bot, and says to
  restart it. The version is a hash of their sources outside tests (the probe, the bot,
  conditions, framing, launch), not of the protocol crates. A client tells it in every answer, from
  the moment its socket opens, even while it loads.
- The bot renders translated chat from the game's `en_us.json`, read from the jar with `unzip`.

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
