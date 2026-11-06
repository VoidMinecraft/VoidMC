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
mannequin riding the display entity. A display entity turns as it glides. A rider's view
takes its pitch from moves, which the client eases over three ticks, and its yaw from its
head, which only comes as a byte: `EntityPositionSync` and `SetHeadRotation` once a tick
make a smooth turn, two small packets for each tick of the turn. The player's
`walking_speed` undoes the wider view a client gives someone flying: an eye and a camera
see the same thing.

How the view goes is a `Flow`, chosen apart for the way in and the way out, and an outro
is the flight of its intro, backwards. All end exactly in the eye, or on the view:

- `/map intro 1`, `Envol`: straight up, tilting down to the ground under the player, then
  back from straight over their marker.
- `/map intro 2`, `Spirale`: up a widening spiral, turning a whole circle, then out of one
  on the model.
- `/map intro 3`, `Vol plané`: ahead and up like a bird, then in from low over the land.
- `/map intro 4`, `Saut dans le ciel`: eyes on the sky and up into it, then down from low
  over the land onto the view.
- `/map intro 5`, `Soucoupe`: a film, see below, with the flight of `Envol` on the model.

`/map outro` takes the same numbers. Either one, from inside the map, also plays that flow
over without leaving: the world stays on the client, so that an eye flies over it while
the player stays on the rig, and a veil covers each cut. `PreviewFlow(player, part, flow)`
does that for anything else, and changes no choice: that is
`ChooseLook(player, LookChoice::Intro(Some(flow)))`, or `Outro`.

### The saucer

`Soucoupe` is seen from outside. On the way in, the camera leaves the head through its top
and draws back, a saucer comes over the player, its beam takes them up struggling, and it
goes off into the sky with the camera after it. On the way out it comes down from the sky,
the beam lets them down, they land dizzy, it leaves, and the camera comes back over the
head and down into the eye.

`abduction.rs` says what happens at each tick of a `Scene`, and `plugin::abduction` plays it
for any `Flight` that has one, real or preview. So that is all it takes for both, and
nothing of it is in the passages: the scene starts when such a flight does and takes its
clock from it. It ends when the camera goes, for whatever reason, which covers a player
leaving midway. What it is made of goes out to all who see the chunk, like the cameras.

- A client does not draw its own player while it looks through something else, so there
  is one body on screen, a mannequin with the account's skin. It draws the inside of a head
  too: the body only exists while the camera is out of it, and over the head the look stays
  level for it to be out of frame when it comes and goes.
- A body is not a rider, which sits and stops its limbs: a client swings the limbs of what
  moves along the ground, so it goes round the beam by a move each tick, and glides over
  three ticks as any living entity. Its `Stance` adds what a client animates by itself: the
  shaking of the frozen, the pose of an elytra, in which the pitch turns the whole body,
  and the spin of a riptide.
- The saucer and the beam are item displays of the pack and glide by segments, as cameras
  do. The beam is one sheet that the client turns to the camera, and it grows by
  interpolation. A client with no pack gets a pressure plate and a glass pane.
- The film is shot from the first of four sides with room for the camera, in the blocks
  around the player. It settles that as the flight starts, which still leaves the eye or
  the sky. With none, it goes up as far as the land asks. The saucer still comes through
  any roof.
- The veil holds a little longer on the way out, or the sections a client fades in would
  show around the player for as long as the camera stays up there.

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
  tick whatever the camera is, and those differences slide the camera, through the dead
  zone of `motion`: `Motion::Slack`, the only way a player gets. The other three and
  inertia keep their code and their tests, but nothing offers them any more. The rim is soft: past it a view slows down over `PAN_MARGIN` blocks
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
nothing. The client still redraws the sections around its new place, which takes it some
50 ms with a render distance of 8. Those sections would then fade in from the colour of
the fog over `chunkSectionFadeInTime`, most of a second of a dark map, so the pack
overrides `chunksection.glsl` for them to be whole at once, east of `FIRST_MODEL_BLOCK`
only. What is left of a threshold is those 50 ms, with the sky in the first frames, and
markers that come back a moment later, since they are new entities. A camera glides some
ticks behind what it was told, and a jump does not: in the middle of a zoom, it goes to the
height the client was showing, and the glide takes over from there, or the whole view would
leap ahead.

Chunks are built and encoded once, on start for the default look and clouds, and on
first use for the others. In the map a client is told to keep a far larger radius with
`SetChunkCacheRadius`, which does not change what it renders, around a centre that never
moves, and the server's radius again on the way out. So a chunk it has seen does not go
out twice.

What a player's chunks do in a tick is held to `CHUNKS_TIME_PER_TICK`: a chunk built for
the first time, a repaint or a chunk of the real world cost far more than a chunk that is
ready, and past that time the rest waits for the next tick. All the empty chunks past the
rim are one chunk in the caches, which a pan would otherwise build one by one on its ticks.

## Looks

The engine's command arguments form one chain with no sub-literals, so one option
follows any action, and one that does not fit the action is refused. Each action alone
moves on to the next choice. Choices last for the connection and apply at once. Their
name shows for a moment over the hotbar, not in the chat.

Pressing sprint in the map opens the same choices in a dialog: every setting on one screen,
the choice that is on in gold, and a click that applies it at once and keeps the dialog
open, which the server sends again with the new choice. Escape or Fermer closes it. The
game does not pause, so the map goes on behind it, streams, and changes as choices do. A
client blurs what is behind a screen, by an option the server has no say in, so the pack
replaces the blur effect with nothing, and draws the shade and the buttons so that the map
shows through. All three go for every screen of the client while the pack is loaded, and
a client that refused the pack keeps its own and a blurred map. The mouse has nothing to
catch up with on closing: the client puts its pointer back in the middle and ignores the
next move. The first row under the settings plays an animation, see below.

The mouse never leaves the middle of the screen in the map, so a button drawn on the view
could not be pointed at. The way to the dialog is the key that the hint names, or Réglages
carte in the client's own menu, on Escape: a dialog in the `minecraft:dialog` registry and in
the tag `minecraft:pause_screen_additions`. The client opens that one on its own, so it
shows no choice as on until the first click brings the real one. Off the map, a click on it
closes it. Fermer goes back to the menu it came from.

The wheel of `wheel.rs` is out of reach behind `WHEEL_OPEN`, until the dialog is settled.
It opens while sprint is held, drawn in the pack: the mouse across turns between Panneau,
Marqueur, Déplacement, Zones and Blocs, up and down steps through the choices, which apply
as they go. A look turns on the client, so it swaps its eye for the same mannequin as a
flight, which keeps the height of an eye. The player rides under it, right where they stand:
a client whose camera is another entity only reports its mouse while riding.

### Adding a setting

`settings()` in `settings.rs` is the only list. One line adds a row to the dialog:

```rust
Setting::new("Météo", &Weather::ALL, Weather::label, |looks| &mut looks.weather),
```

`Weather` is any `Copy + PartialEq` type with its choices in a static slice and a name for
each, and `MapLooks` holds its value. That is all it takes for the buttons, the click and
what the choice is kept in. `session.looks` then reads it. What has to happen at the moment
it changes, such as a respawn of entities, goes in `choose_look`, next to `before.me`.
A setting of more than four choices takes several rows.

### Previews

The dialog also offers to watch what is otherwise only seen on leaving: `PREVIEWS` in
`settings.rs` lists them, and `play_preview` in `plugin.rs` plays them through the same
`Flight` and veil as the real thing, on the model. The dialog makes way and comes back when
the view is back where it was. `Ending::Rehearsal` is a flight that ends under the veil, with
the view back in place behind it, and `Cut` starts a flight elsewhere than the view by
moving the camera under the veil first. A new one takes a line in `PREVIEWS` and an arm in
`play_preview`.

A choice only touches what it changes. A panel is one entity, a marker a few. A look of
the chunks reaches what the client holds as block changes, around the view first, by
`REPAINTED_BLOCKS_PER_TICK`: no chunk leaves, none is sent again, and going through
choices faster than that only sends what ends up different. A zone leaves the open land
alone. The chunks still to come go out in the new look.

- `/map style 1` to `4`: a full sheet, text alone, a round bubble with the name under it,
  or a large title over a flourish. `/map pan 2` opens exploration with that style.
- `/map me 1` to `3`: the player's own marker, flat and facing the camera: a small pin, an
  arrow that turns to where they were looking, or their face on a disc with a small arrow
  under it. A face is a text object, so it stays a picture. Only the account id goes out,
  so an offline account shows the default skin of its id. `Me::ALL` lists them, each as a
  `LookChoice::Me`.
- `/map zone 1` to `4`: closed regions, under small clouds of blocks, with diagonal
  stripes, behind a fence with a lock, or drained of colour with a large question mark.
  A cloud is a few round puffs of white blocks on one to three flat layers, as in a game.
  Its shape comes from a fixed seed and where it hangs, so no two are alike. It is drawn in
  pixels of the world, on the grid of the overview's cells, so that every model draws the
  same pixels: a cell a pixel there, a cell or two on the middle model and two or three up
  close. It hangs at the same height in the world, on a model's own scale, as a cloud
  closer to the eye shows larger. It is built with the model, not on opening the map.
- `/map border 1` to `3`: what stands along the border of a closed region under
  `/map zone 3`, on a band three blocks wide on the close model and as wide on screen on
  the others: a wall with a merlon every other block, the same wall with square towers, or
  a wall of coloured glass. Two closed regions share one band, on the side of the more
  closed one. The camera looks almost straight down, so a design is its top, drawn for the
  cells a model has: a wall of three cells has a walkway between two parapets, one of a
  single cell is nothing but its merlons, and a tower is never under three cells wide. A
  merlon is one cell on every model, and one in two by the parity of its column, which
  holds whichever way the band turns. What stands on a band keeps to the level of the land
  around it, or it would go up and down every terrace.
- `/map weather 1` to `6`: what is in the air, `Weather::ALL` in order. First and by default
  the rain and snow of the client, then flakes and drops over the regions that have them,
  leaves in the wind over the woods, embers over the closed regions, glints over the snowy
  ones, or nothing. A region has a `Climate`, which is also the biome of its columns. The
  client draws its own rain and snow only around the camera, whatever is under the middle of
  the screen, so they cover the whole view and change as the camera crosses a border.
  Particles stay over their land, wherever it is on screen: they hang on the line from the
  eye to the land, close to the eye, or they would be smaller than a pixel, and a coarser
  model sends more of them for as many to show. An `Effect` in `weather` says which land,
  how many, how far from the eye and which particle, and that is all a new one takes. The
  land, the sky and the fog keep their colours under any. A choice sends no block.
- `/map move libre` or `sauts`: the view slides under the mouse, or goes from point to
  point, see below. Déplacement in the dialog is the same choice.
- `/map blocks flat` or `real`: the flat colours of a printed map, or the blocks of the
  world with their textures. Their tint comes from the `voidmc:map` biome. Real water is
  translucent, and the client draws it after a text seen through the land, which writes no
  depth: over water a card comes out tinted. Without see-through, the card's own letters
  fall behind its background, so that stays as it is.

## Going by jumps

With `/map move sauts` the mouse no longer slides the view. `goto` holds where it is going,
and `plugin::goto` feeds that from the session. A target is the player's own marker, a
character or a waypoint: the view glides to it and keeps it in the middle of the screen, at
any zoom and on any model, for as long as it is the target. So someone walking or sailing
is followed. The boss bar names the target. What ends that is another target, or a right
click, which also leaves the view where it is.

- A flick of the mouse goes to the nearest point that way, within 60° of it, and one
  gesture is one jump, however long: the mouse has to rest for a quarter of a second.
- Forward and back go down and up the list, around, from where the target is in it.
- Jump opens the list, a dialog of every point: `Moi`, the characters and the waypoints,
  the closed ones in grey. A click or Enter on a row goes there, and the list stays open
  to try another. Escape or Fermer closes it.
- Left and right zoom, during a glide as well. A click on a waypoint in the middle works
  as it does anywhere, up to a dive.

The glide is a `SmoothDamp` towards the target, recomputed each tick: it starts and ends on
a curve, keeps its speed if the target changes, and stays under `MAX_VIEW_STEP`. A pan goes
by the sea, so a target that stands high is aimed at along the ray of the middle of the
screen, or a marker would come out north of it.

The client sends nothing when Tab is pressed: `keyPlayerList` is only read by `Gui`, to draw
its list. It cannot show a choice either, as it takes no click. So the keys of jumps are
those of `PlayerInput`, and Tab has two other uses. Held on the map, it shows the list the
server fills: the characters by name, with a display name that only that list shows, and
who the view is on in its footer. In a dialog the key belongs to the screen: it moves down
the rows, scrolling them as it goes, as the arrows do, and Enter presses the row. A move
of that focus sends nothing either, so the view only moves on Enter.

The client scrolls a dialog that does not fit, by itself: the buttons are in a scroll area
as high as the screen allows, with a bar, which the wheel moves. Sending the dialog again
would put the scroll back to the top and lose the focus, so a click on a row sends nothing,
and the list does not say which row is the target: the boss bar and the view do.

A dialog cannot stand on one side: the client centres every element, and the only cell a
grid has is a button, which has a sprite and takes a stop of the Tab key even with no action.
So the list stays in the middle, narrow, and the view makes way: while it is open the view
stands `LIST_SHIFT` of its height west of the target, which shows right of the list. Hovers
then go by the target instead of the look, so that its card or its name stay up.

## Travelling

A first left click on a marker selects it, which shows its status and description. A
second one dives into it: over `DIVE_TICKS` a camera entity flies down to it along the
look, starting and ending at rest, and the player then lands at the real place,
`ARRIVAL_CLEARANCE` above the ground. In a look, that entity takes over from the eye.
Crouching calls the dive off. A closed region refuses to be travelled to, and says so.

A spectator only clicks when the look hits an entity. So an `interaction` surrounds the
eye: it is always hit, and it has no model, so that nothing can show of it, even to a
spectator, who sees invisible entities. A left click sends `SpectateEntity`, a right click
sends `Interact`. A client with no pack keeps a chest minecart for its crosshair, and
neither box exists during a flight, which takes no clicks.

The model fits a client render distance of 8. Below that its far rim is cut. The last
13 blocks of a render distance fade out, which the server cannot change: both views
keep what matters closer than that.

Regions are data in `regions`: each is discovered, undiscovered or locked, and the model
only takes the colours of a region down a little to show it. `Regions::demo` is a fixed
example.

A waypoint of a closed region is the item of its place, drained of colour, with a small
gold lock or a question mark in its corner: the place a player will know once it opens,
and closed at a glance. An item model can only tint, which only darkens, so the pack brings
the item shader of the game with one change: it turns grey what is drawn with the tint of
`DRAINED_TINT`, the one of these models. A client without the pack gets the usual item,
darker. The sign of a region stands clear of its waypoints.

## The pointer

The pointer is a title of one glyph, from a font of its own. The client centres a title
at any size of window, and draws it four times larger: sixteen pixels of the picture at
a height of 4 make one pixel of the picture one pixel of the gui, as in the crosshair,
so it stays sharp at every gui scale. A change of look is a new text, with no fade.

The client's crosshair stays as it is. A spectator only sees it over an entity with a
menu, so the `interaction` around the eye leaves it off, and nothing has to tell the map
from the rest of the game. A client that refused the pack gets a chest minecart, and its
own crosshair.

The veil is a title too, and new times or a new text would cut it short. So the pointer
gives way to it, and only comes back once it has gone. During a dive and while the wheel
is open, it is cleared. Behind the settings it stays, with what it is over.

Replacing the crosshair sprite is not an option: the pack applies everywhere. A core
shader can tell the map apart, from `CameraBlockPos` in `Globals`, which every program
gets, and a hover from a band of `GameTime`. But the crosshair blends with `INVERT`, so
its colours come out as a mix with what is under it, and only inverted shapes are
possible. The action bar is placed from the bottom of the screen, so it only reaches the
middle with a shader on all text, and it fades unless sent again.

## Characters

`Characters` lists who the map shows: a name, a skin, a place in the real world, a yaw
and whether they are on the water. It does not say who plays them. For now `Walk` fills it with `VOID_MAP_WALKERS`
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
model. A figure comes with a player info entry with no name: the client
needs one to show it, and an empty name leaves nothing over its head. It is listed under a
display name, which only the tab list shows. Its id is made so
that it hashes like that of its skin, which is how a client picks a default skin.

A figure is the same share of the land on every model, `LARGER_THAN_LIFE` times the true
scale, so it follows the zoom as a block does, and a model taking over shows it the size
it was. This is why it is not a mannequin: vanilla draws an entity up to 64 blocks times
the size of its box, and so nothing smaller than about 20 pixels on a 760 pixels window,
whatever the zoom. A player on the client, `RemotePlayer`, goes ten times as far.

A model taking over gets figures of its own, and boats, rather than moving those of the
last one across: a client glides an entity to where it was told, and a jump past 64 blocks
leaves what it was gliding to in place. An entity then flies back towards the last model,
and a boat ends up stopped on the empty chunks between two models, where nothing ticks. In
the same tick as the camera they are simply there, the same share of the land away.

The player sees themselves too, on the ground under their marker: a figure for that
session alone, with the skin of their account, as large as the others and with an id of
its own, or the client would take the entity for itself. Seen from nearly straight above,
a marker would cover it from any height, so the marker goes up the screen as a billboard
does, to stand on its head. The look never names it: the marker is there for that.

A character can be `afloat`. On a lake, every third walker takes a boat: `lakes_of` finds
the water with at least two `Landing`s, each a spot of ground a few cells from it, walks
them to the closest one, and sends them `CROSSINGS` times across before they land and walk
on to a place. Water is searched like ground, in a `Ground` of its own, and bridges close
a lake off. Some start on the water, so that boats show from the first moment.

A boat is an item display of a model of the pack, not a `minecraft:oak_boat`: an entity of
the game has no scale and is drawn no further than a mannequin. A client without the pack
gets the flat sprite of the boat. The model has a mast ahead of its rider and a small sail
the wind fills away from them, for it to show from nearly straight above. The figure rides it, which sits it down, and its body
follows the boat with no move of its own. A rider hangs under its vehicle, so the display
stands higher than it looks, for the feet to stay in the top block of the water: a client
shows nothing of an entity whose feet are in a section it cannot see.

The look never reaches a figure, since it hits the box around the eye first, and clicks
only go to markers. So the name is decided on the server like a hover of a marker, within
about the figure's body, and never close to a marker, which always comes first: the
figure glows, with a text display over it.
`map characters` in the logs says what all this costs, every 200 ticks.

## Scenes of life

`life` plays what goes on around the characters: gulls in small flocks that fly about and
land, a train going round its line with a stop at each station, someone fishing from a pier,
someone out for a walk with balloons. Animations in the dialog turns them all off.

A `LifeScene` is data, and it serialises: its `way`, a spot, a route of turns or a zone to
roam over, its `cast`, the models and how each follows the way, and the `marks` it leaves on
the map, rails or a pier. A hand-written one, which the tests read:

```json
{
  "name": "Le ballon de Lou",
  "way": {"Route": {
    "turns": [{"at": [0, 0]}, {"at": [40, 0], "rest": 60}],
    "ends": "BackAndForth",
    "speed": 0.2
  }},
  "cast": [
    {"model": "Stroller", "size": 14.4},
    {"model": "Balloons", "size": 20, "up": 12, "behind": 1.5, "bob": 0.5}
  ]
}
```

All of it is in world blocks and ticks, whatever the model. `Playback` is the only way to
play a scene, and `Land` all it asks of a map: the height of the ground, water, where a place
of the world falls on the model. It reads no generator, which a test holds it to. A map made
from the world gets its scenes from `life::habitat`, which reads them off a `Land`, and
`build_atlas` hands them on. A map made by hand has a file instead, and from there it is the
same road: `Scene::lay_all` draws the marks on each model, `Life::new` plays them.

Where an actor is at a tick is a function of that tick, with no state, so nothing runs for a
scene nobody looks at, and looking ahead is as cheap as looking at now. An actor is an item
display of a model of the pack, told every `STEP_TICKS` where it will be `GLIDE_TICKS` ahead:
the client glides there at a steady pace, and the next place comes before it arrives. That is
two moves a second for what moves, and none for what stands still. A gull beating its wings,
the smoke of the locomotive and the rings around a float are animated textures, which a client
plays by itself: all the copies of a texture go together, so a flock takes three, out of step.

A scene whose actors are out of sight of the eye has no entity. A model taking over gets new
ones, as figures do. Sizes are in world blocks, so that an actor follows the zoom as a block
does, with `least`, in blocks of the model, for what would be under a pixel on the far one.
Rails are a ladder of sleepers on the close model and a plain line on the others.

`map life` in the logs says what all this costs, every 200 ticks.

## Layout

The core has no packets and no ECS: `window`, `terrain`, `noise`, `regions`, `scene`,
`fence`, `weather`, `buildings`, `markers`, `pointer`, `motion`, `goto`, `transition`,
`abduction`, `wheel`, `art`, and `life`, apart from `life::show`. `biome` declares the
level, `plugin` runs the sessions, `plugin::preview` plays a flow over,
`plugin::abduction` plays the film of the saucer, `plugin::goto` moves a view by jumps,
`settings` lists what can be changed, `players` shows the characters, `cursor`
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
