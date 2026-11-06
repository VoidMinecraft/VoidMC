# void-map

A POC of a full-screen server map. `/map` takes the player to a block model of the
world, and aims at its markers with the mouse. Crouching brings them back.

The client renders the model itself: it is made of real chunks, sent to this player only,
far out in their own dimension, in a private `voidmc:map` biome whose attributes turn the
sky black wherever the camera is. No respawn, so no loading screen and no emptied world.
A small resource pack draws the info panels. A client without it gets plain text.

## Going there and back

The camera rises out of the player's eyes on an entity of its own, into a veil of cloud:
a title made of large glyphs, the one thing the server can cover the screen with and
fade. Under it the world's chunks go and the model's come, and the veil lifts once the
client has them. The way back is the same in reverse: the veil comes at the end of a
dive, or when crouching, stays until the real chunks are all there, and lifts over a
camera coming down to the player's eyes.

The sky only goes nearly black: by day a dark brown stays, and at night the moon and the
stars show under the model. The level's clock has the last word over the biome there.

A real preload is out of reach: the client only keeps a square of chunks around its
centre, so the model's anchor and the destination cannot both be in it. Nor can the
model sit over the real columns: each player would need chunks of their own, made of
both, instead of packets encoded once.

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
a zoom goes from the detail model to a middle one and to the overview, whose blocks are
larger each time, at the heights in `pointer`.

The three models sit in a row on the client, far enough apart for a camera to jump from
one to the next instead of gliding, and at a scale that keeps the middle of the screen
in the same place at the same size. The chunks of the models next to the view go out
during the pan, after those of its own, so a threshold moves the camera and sends
nothing. The client still redraws the sections around its new place.

Chunks are built and encoded once, on start for the default look and clouds, and on
first use for the others. In the map a client is told to keep a far larger radius with
`SetChunkCacheRadius`, which does not change what it renders, around a centre that never
moves, and the server's radius again on the way out. So a chunk it has seen does not go
out twice.

## Looks

The engine's command arguments form one chain with no sub-literals, so one option
follows any action, and one that does not fit the action is refused. Each action alone
moves on to the next choice. Choices last for the connection and apply at once. Their
name shows for a moment over the hotbar, not in the chat.

Holding sprint in the map opens the same choices on a wheel, drawn in the pack: the mouse
across turns between Panneau, Marqueur, Zones and Blocs, up and down steps through the
choices, which apply as they go. The camera holds still, and letting go keeps the choice.
A look turns on the client, so the wheel swaps its eye for an entity: the client widens
the view of a flying player and not of an entity, which stands further back to make up.

- `/map style 1` to `4`: a full sheet, text alone, a round bubble with the name under it,
  or a large title over a flourish. `/map pan 2` opens exploration with that style.
- `/map me 1` to `8`: the player's own marker, as a small pin, an arrow on the ground that
  points where they look, a ring on the ground, their head on a small disc, their head
  with a small arrow right under it, on a bubble that points down, over a ring, or over
  the arrow. Only the account id goes out, so an offline account shows the default skin
  of its id.
- `/map zone 1` to `4`: closed regions, under small clouds of blocks, with diagonal
  stripes, behind a fence with a lock, or drained of colour with a large question mark.
  A cloud is one of a few drawings of up to three flat layers, as in a game, and hangs
  at the same place of the world on every model. It is built with the model, not on
  opening the map.
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

## Characters

`Characters` lists who the map shows: a name, a skin, a place in the real world and a
yaw. It does not say who plays them. For now `Walk` fills it with `VOID_MAP_WALKERS`
walkers, 20 by default, who go on foot between waypoints. To show connected players, a
system copies them into the same list, with their account's profile as the skin.

Walkers keep to the ground of the detailed model: land and bridges with room around them,
clear of trees, landmarks and the rim, and with no step higher than a terrace next to
them. Ways are searched on that ground once per place, and each step checks it again: a
walker turns back rather than leave it. The two coarser models draw the shores and the
trees a little elsewhere, so there a figure stands on the closest free cell, and goes
unseen if there is none near.

Each one is a `minecraft:mannequin`, in each session, placed from its world position
onto the current model. A move goes out every tick, the client glides over three, and it
turns and swings the legs from how the entity moves. The legs hardly swing in a look:
the client reads them from blocks per tick, and a walk is a share of a block on a model.

Figures are not at the scale of the model. Vanilla draws an entity up to 64 blocks times
the size of its box, so at a true scale they would vanish far closer than the camera
stands. Each takes the size its distance calls for, in steps, which also keeps them the
same size on screen. `DRAWN_MARGIN` sets how large that is.

The look never reaches a figure, since it hits the box around the eye first, and clicks
only go to markers. So the name is decided on the server like a hover of a marker, within
about the figure's body, and never close to a marker, which always comes first: the
figure glows, with a text display over it.
`map characters` in the logs says what all this costs, every 200 ticks.

## Layout

The core has no packets and no ECS: `window`, `terrain`, `noise`, `regions`, `scene`,
`buildings`, `markers`, `pointer`, `transition`, `wheel` and `art`. `biome` declares the
level, `plugin` runs the sessions, `players` shows the characters, and `telemetry` measures
them.

## Logs

`RUST_LOG=voidmc_map=debug` adds structured events for late ticks, gaps between the
client's reports, looks past the limits, slow chunk sends, zoom keys and model changes.
Entering, leaving, modes, hovers and clicks are at `info`, along with a summary of each
session every 200 ticks.

`tools/stutters.py` lines up, by timestamp, the frame spikes of a client's
`client-perf.log`, the pauses of a Java Flight Recorder file and these events of the
server, and gives each spike a probable cause.

## Running it

```sh
VOID_MAP_ADDRESS=127.0.0.1:25610 VOID_MAP_PACK_ADDRESS=127.0.0.1:25611 cargo run -p voidmc-map
```

Then `/map` in game, jump, `/map pan` or `/map look` to change mode, and crouch or
`/map stop` to leave. Left click selects the marker in the middle of the screen, and
right click clears the selection.

The model covers one fixed area around the origin. Its markers come from
`MARKERS`, and only those on the model are shown.
