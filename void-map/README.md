# void-map

A POC of a full-screen server map. `/map` takes the player to a block model of the
world, and the mouse drives a cursor on it. Crouching brings them back.

The client renders the model itself: it is made of real chunks, sent to this
player only, in a dedicated `voidmc:map` dimension with a black sky. No resource
pack is needed.

## How the view works

The camera does not follow the mouse.

- The player is a spectator riding an invisible entity, and `SetCamera` shows the
  game from another entity, fixed and tilted.
- A rider reports its rotation every tick whatever the camera is. Only the
  differences between reports count, and they move a cursor on the model.
- Past 45° of pitch the player is turned back with a relative `PlayerRotation`.
  The view does not move, so this is invisible and loses no movement.
- When the cursor nears the edge of the view, the camera entity moves.
- An `Interaction` box surrounds the camera, so its look always hits something. A
  left click sends `SpectateEntity`, a right click sends `Interact`.

## Layout

The core has no packets and no ECS: `window`, `terrain`, `scene`, `markers` and
`pointer`. `dimension` declares the level, and `plugin` runs the sessions.

## Running it

```sh
VOID_MAP_ADDRESS=127.0.0.1:25610 cargo run -p voidmc-map
```

Then `/map` in game. Left click selects the marker under the cursor, and right
click clears the selection.

The model covers one fixed area around the origin. Its markers come from
`MARKERS`, and only those on the model are shown.
