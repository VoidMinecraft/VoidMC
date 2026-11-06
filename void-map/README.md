# void-map

A POC of a full-screen server map. `/map` takes the player to a block model of the
world, and aims at its markers with the mouse. Crouching brings them back.

The client renders the model itself: it is made of real chunks, sent to this
player only, in a dedicated `voidmc:map` dimension with a black sky. A small resource
pack draws the info panels. A client without it gets plain text.

## How the view works

The middle of the screen is the pointer. The hovered marker is the nearest in angle
from that ray, under a threshold, and it holds on a little past it so that the choice
never flickers. A click acts on that marker and nothing else.

- `/map` or `/map look`: the overview, with the whole island on screen. The player
  looks around, and the client turns the view with no delay. They stand south of the
  model, so straight down is off the land. `REST_PITCH` sets the angle, and the height
  of the eye follows from it, so that the land stays centred in the frame. Nothing
  corrects the look while the mouse moves: it can leave the island by a wide margin, and
  once the mouse rests it drifts back by small relative steps. There is no stop to hit.
- `/map pan`: exploration, on a model about three times larger than the overview, which
  streams around the camera. The player rides an invisible entity, and `SetCamera` shows
  the game from it, with a look that never changes. A rider reports its rotation every
  tick whatever the camera is, and those differences slide the camera. The rim is
  elastic: past it the view resists over `PAN_MARGIN` blocks, and comes back once the
  mouse rests. The view eases to a stop, which lets the camera glide over three ticks
  without overshooting. Past 45° of pitch the player is turned back with an absolute
  `PlayerRotation`: the client answers with exactly that pitch, which tells the
  recentring from mouse movement however fast the mouse goes.

The jump key changes mode, and the boss bar names the mode. The hint on screen names the
player's own keys. In exploration the left and right keys zoom for as long as they are
held, read from `PlayerInput`. A render distance cannot show the land from far above, so
past `COARSE_ABOVE` the view moves to the overview model, whose blocks are larger, and
back under `DETAIL_BELOW`.

Chunks are built and encoded once, on start for the default look and on first use for
the others. In the map a client is told to keep a far larger radius with
`SetChunkCacheRadius`, which does not change what it renders, and the server's radius
again on the way out. So a chunk it has seen does not go out twice in a pan.

## Looks

The engine's command arguments form one chain with no sub-literals, so one option
follows any action, and one that does not fit the action is refused. Each action alone
moves on to the next choice. Choices last for the connection and apply at once.

- `/map style 1` to `4`: a full sheet, text alone, a round bubble with the name under it,
  or a large title over a flourish. `/map pan 2` opens exploration with that style.
- `/map me 1` to `4`: the player's own marker, as a small pin, an arrow on the ground that
  points where they look, a ring on the ground, or their head on a small disc. Only the
  account id goes out, so an offline account shows the default skin of its id.
- `/map zone 1` to `4`: closed regions, under clouds of blocks, with diagonal stripes,
  behind a fence with a lock, or drained of colour with a large question mark.
- `/map blocks flat` or `real`: the flat colours of a printed map, or the blocks of the
  world with their textures. Their tint comes from the `voidmc:map` biome.

## Travelling

A first left click on a marker selects it, which shows its status and description. A
second one dives into it: over `DIVE_TICKS` a camera entity flies down to it, starting
and ending at rest, and the player then lands at the real place, `ARRIVAL_CLEARANCE`
above the ground. In a look, that entity takes over from the eye, looking the same way.
Crouching calls the dive off. A closed region refuses to be travelled to, and says so.

A spectator only clicks when the look hits an entity, and only sees the crosshair when
that entity has a menu. So a chest minecart surrounds the eye, which sits just under
its floor: the cart is always hit, and looking down it never shows. A left click sends
`SpectateEntity`, a right click sends `Interact`.

The model fits a client render distance of 8. Below that its far rim is cut. The last
13 blocks of a render distance fade out, which the server cannot change: both views
keep what matters closer than that.

Regions are data in `regions`: each is discovered, undiscovered or locked, and the model
only takes the colours of a region down a little to show it. `Regions::demo` is a fixed
example.

## Layout

The core has no packets and no ECS: `window`, `terrain`, `noise`, `regions`, `scene`,
`buildings`, `markers`, `pointer` and `art`. `dimension` declares the level, `plugin` runs
the sessions, and `telemetry` measures them.

## Logs

`RUST_LOG=voidmc_map=debug` adds structured events for late ticks, gaps between the
client's reports, looks past the limits, slow chunk sends, zoom keys and model changes.
Entering, leaving, modes, hovers and clicks are at `info`, along with a summary of each
session every 200 ticks.

## Running it

```sh
VOID_MAP_ADDRESS=127.0.0.1:25610 VOID_MAP_PACK_ADDRESS=127.0.0.1:25611 cargo run -p voidmc-map
```

Then `/map` in game, jump, `/map pan` or `/map look` to change mode, and crouch or
`/map stop` to leave. Left click selects the marker in the middle of the screen, and
right click clears the selection.

The model covers one fixed area around the origin. Its markers come from
`MARKERS`, and only those on the model are shown.
