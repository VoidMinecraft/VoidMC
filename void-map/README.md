# void-map

A POC of a full-screen server map. `/map` takes the player to a block model of the
world, and aims at its markers with the mouse. Crouching brings them back.

The client renders the model itself: it is made of real chunks, sent to this player only,
far out in their own dimension, in a private `voidmc:map` biome whose attributes turn the
sky black wherever the camera is. No respawn, so no loading screen and no emptied world.
A small resource pack draws the info panels. A client without it gets plain text.

## Going there and back

`/map` opens at once: the player turns spectator, which hides the hand, and the model
starts coming, closest to their marker first, while the camera rises straight up out of
their eyes. A short veil of cloud, a title made of large glyphs, covers the cut. Under it
the camera is already over the player's marker on the model, close up and in the middle
of the screen, and from there it draws back to the view without a break. The way out
is the same one backwards, down to the marker or a waypoint, through the veil, and from
the sky of the world down to the player's eyes.

There is nothing to wait for under the veil, because the client never lets go of the
world. It keeps every chunk in a square around a centre, whose size the server sets: the
models sit only a few dozen chunks east of what the map shows, and a `Site` is a square
that holds both. The real chunks stay on the client and in `LoadedChunks`, so that block
changes keep reaching it, and the chunks of a waypoint go out as soon as it is selected.
Only `RENDER_GAP` keeps the two from showing on each other's horizon. Whoever walks
out there, past 1300 blocks east, would see the entities of a model: chunks are per
player, but the entities on them are not.

Cameras are display entities: they are told the end of a segment, `SEGMENT_TICKS` ahead,
and the client glides them there, which takes a handful of packets for a flight. The
next segment arrives before the last one ends, so that they never stall. At the end they
get their exact place, since the engine rounds angles, and come to rest before anything
takes over from them. Going from a player to an entity without an eye would move the view
by the height of one, so a flight from or to the eye is seen through an invisible
mannequin riding the display entity. A rider cannot turn smoothly, so flights keep the
look they start with. The player's `walking_speed` undoes the wider view a client gives
someone flying: an eye and a camera see the same thing.

The sky only goes nearly black: by day a dark brown stays, and at night the moon and the
stars show under the model. The level's clock has the last word over the biome there.

The model cannot sit over the real columns: each player would need chunks of their own,
made of both, instead of packets encoded once.

## How the view works

The middle of the screen is the pointer, which the pack draws and which turns gold over
a marker. The hovered marker is the nearest in angle
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
  tick whatever the camera is, and those differences slide the camera, in one of the
  ways of `motion`. The rim is soft: past it a view slows down over `PAN_MARGIN` blocks
  and stops where it stops, with nothing stored up, so it never springs back and turning
  round answers at once. A view eases to a stop, which lets the camera glide over three
  ticks without overshooting. Past 45° of pitch the player is turned back with an absolute
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
across turns between Panneau, Marqueur, Déplacement, Zones and Blocs, up and down steps
through the choices, which apply as they go. Its middle shows the choice between the ones
above and under it, with arrows for the mouse, and letting go keeps it. The camera holds
still, and a marker under the look stays hovered, for its panel to show the styles.
A look turns on the client, so the wheel swaps its eye for the same mannequin as a flight,
which keeps the height of an eye. The player rides under it, right where they stand: a
client whose camera is another entity only reports its mouse while riding.

A choice only touches what it changes. A panel is one entity, a marker a few. A look of
the chunks reaches what the client holds as block changes, around the view first, by
`REPAINTED_BLOCKS_PER_TICK`: no chunk leaves, none is sent again, and going through
choices faster than that only sends what ends up different. A zone leaves the open land
alone. The chunks still to come go out in the new look.

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
- `/map move a` to `d`: how a pan follows the mouse, to compare them. `a` is a critically
  damped spring, `b` a dead zone, `c` looks ahead by the speed of the mouse, `d` predicts
  between two reports. `/map move inertie` makes any of them go on for a moment once the
  mouse stops. Twice the mouse is twice the distance in all of them.

## Travelling

A first left click on a marker selects it, which shows its status and description. A
second one dives into it: over `DIVE_TICKS` a camera entity flies down to it along the
look, starting and ending at rest, and the player then lands at the real place,
`ARRIVAL_CLEARANCE` above the ground. In a look, that entity takes over from the eye.
Crouching calls the dive off. A closed region refuses to be travelled to, and says so.

A spectator only clicks when the look hits an entity. So a minecart surrounds the eye,
which sits just under its floor: the cart is always hit, and looking down it never
shows. A left click sends `SpectateEntity`, a right click sends `Interact`.

The model fits a client render distance of 8. Below that its far rim is cut. The last
13 blocks of a render distance fade out, which the server cannot change: both views
keep what matters closer than that.

Regions are data in `regions`: each is discovered, undiscovered or locked, and the model
only takes the colours of a region down a little to show it. `Regions::demo` is a fixed
example.

## The pointer

The pointer is a title of one glyph, from a font of its own. The client centres a title
at any size of window, and draws it four times larger: sixteen pixels of the picture at
a height of 4 make one pixel of the picture one pixel of the gui, as in the crosshair,
so it stays sharp at every gui scale. A change of look is a new text, with no fade.

The client's crosshair stays as it is. A spectator only sees it over an entity with a
menu, so a plain minecart around the eye leaves it off, and nothing has to tell the map
from the rest of the game. A client that refused the pack gets a chest minecart, and its
own crosshair.

The veil is a title too, and new times or a new text would cut it short. So the pointer
gives way to it, and only comes back once it has gone. During a dive and while the wheel
is open, it is cleared.

Replacing the crosshair sprite is not an option: the pack applies everywhere. A core
shader can tell the map apart, from `CameraBlockPos` in `Globals`, which every program
gets, and a hover from a band of `GameTime`. But the crosshair blends with `INVERT`, so
its colours come out as a mix with what is under it, and only inverted shapes are
possible. The action bar is placed from the bottom of the screen, so it only reaches the
middle with a shader on all text, and it fades unless sent again.

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

Each one is a `minecraft:player` no client plays, in each session, placed from its world
position onto the current model. A move goes out every tick, the client glides over three,
and it turns and swings the legs from how the entity moves. The legs hardly swing in a
look: the client reads them from blocks per tick, and a walk is a share of a block on a
model. A figure comes with a player info entry, unlisted and with no name: the client
needs one to show it, and an empty name leaves nothing over its head. Its id is made so
that it hashes like that of its skin, which is how a client picks a default skin.

A figure is the same share of the land on every model, `LARGER_THAN_LIFE` times the true
scale, so it follows the zoom as a block does, and a model taking over shows it the size
it was. This is why it is not a mannequin: vanilla draws an entity up to 64 blocks times
the size of its box, and so nothing smaller than about 20 pixels on a 760 pixels window,
whatever the zoom. A player on the client, `RemotePlayer`, goes ten times as far.

The look never reaches a figure, since it hits the box around the eye first, and clicks
only go to markers. So the name is decided on the server like a hover of a marker, within
about the figure's body, and never close to a marker, which always comes first: the
figure glows, with a text display over it.
`map characters` in the logs says what all this costs, every 200 ticks.

## Layout

The core has no packets and no ECS: `window`, `terrain`, `noise`, `regions`, `scene`,
`buildings`, `markers`, `pointer`, `motion`, `transition`, `wheel` and `art`. `biome`
declares the level, `plugin` runs the sessions, `players` shows the characters, `cursor`
draws the pointer, and `telemetry` measures the characters.

## Logs

Built with `--features profile`, each update slower than `VOID_MAP_SLOW_TICK` (25 ms by
default) logs its largest systems, and a summary of them follows every 200 updates.

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
