# void-map

A POC of a full-screen server map. `/map` takes the player to a block model of the
world, and aims at its markers with the mouse. Crouching brings them back.

The client renders the model itself: it is made of real chunks, sent to this
player only, in a dedicated `voidmc:map` dimension with a black sky. No resource
pack is needed.

## How the view works

The middle of the screen is the pointer: the marker nearest that ray is hovered.

- `/map` or `/map look`: the player looks around, and the client turns the view with
  no delay. They stand south of the model and above it, so straight down is off the
  model. Stops hold the look on it, with absolute rotations because reports can be late.
- `/map pan`: the player rides an invisible entity, and `SetCamera` shows the game
  from another one, whose look never changes. A rider reports its rotation every tick
  whatever the camera is, and those differences slide the camera. Past 45° of pitch
  the player is turned back with a relative `PlayerRotation`, which does not show.

A spectator only clicks when the look hits an entity, and only sees the crosshair when
that entity has a menu. So a chest minecart surrounds the eye, which sits just under
its floor: the cart is always hit, and looking down it never shows. A left click sends
`SpectateEntity`, a right click sends `Interact`.

The model fits a client render distance of 8. Below that its far rim is cut.

## Layout

The core has no packets and no ECS: `window`, `terrain`, `scene`, `markers` and
`pointer`. `dimension` declares the level, and `plugin` runs the sessions.

## Running it

```sh
VOID_MAP_ADDRESS=127.0.0.1:25610 cargo run -p voidmc-map
```

Then `/map` in game, `/map pan` or `/map look` to change mode, and crouch or
`/map stop` to leave. Left click selects the marker in the middle of the screen, and
right click clears the selection.

The model covers one fixed area around the origin. Its markers come from
`MARKERS`, and only those on the model are shown.
