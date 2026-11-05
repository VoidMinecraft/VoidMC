# void-map

A POC of a full-screen server map. `/map` takes the player to a block model of the
world, and aims at its markers with the mouse. Crouching brings them back.

The client renders the model itself: it is made of real chunks, sent to this
player only, in a dedicated `voidmc:map` dimension with a black sky. A small resource
pack draws the info cards. A client without it gets plain text cards.

## How the view works

The middle of the screen is the pointer. The hovered marker is the nearest in angle
from that ray, under a threshold, and it holds on a little past it so that the choice
never flickers. A click acts on that marker and nothing else.

- `/map` or `/map look`: the overview, with the whole island on screen. The player
  looks around, and the client turns the view with no delay. They stand south of the
  model, so straight down is off the land. `REST_PITCH` sets the angle, and the height
  of the eye follows from it, so that the land stays centred in the frame. Stops hold
  the look on it, with absolute rotations because reports can be late.
- `/map pan`: exploration, on a model about three times larger than the overview, which
  streams around the camera. The player rides an invisible entity, and `SetCamera` shows
  the game from it, with a look that never changes. A rider reports its rotation every
  tick whatever the camera is, and those differences slide the camera, up to a top
  speed that chunks can follow. Past 45° of pitch the player is turned back with an
  absolute `PlayerRotation`: the client answers with exactly that pitch, which tells
  the recentring from mouse movement however fast the mouse goes.

Space changes mode, Q and D change the height of the exploration view, and the boss
bar names the mode.

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
`buildings`, `markers`, `pointer` and `art`. `dimension` declares the level, and `plugin`
runs the sessions.

## Running it

```sh
VOID_MAP_ADDRESS=127.0.0.1:25610 VOID_MAP_PACK_ADDRESS=127.0.0.1:25611 cargo run -p voidmc-map
```

Then `/map` in game, Space, `/map pan` or `/map look` to change mode, and crouch or
`/map stop` to leave. Left click selects the marker in the middle of the screen, and
right click clears the selection.

The model covers one fixed area around the origin. Its markers come from
`MARKERS`, and only those on the model are shown.
