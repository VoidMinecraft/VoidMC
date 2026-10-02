# Navigation

The `voidmc-navigation` crate moves server-owned entities along computed
paths. Give an entity a `Navigator` and a goal (walk to a point, follow or
flee an entity, patrol a loop), or a `Behaviours` list that picks goals for
it, and the crate does the rest. It plans paths over the voxel world,
steers the entity through the engine physics, and reports how each goal
ended. An optional operator remote shows a mob's path in-world and lets you
drive it.

```toml
[dependencies]
voidmc-navigation = { path = "../void-navigation" }
```

```rust
use voidmc_navigation::{NavRemotePlugin, NavigationPlugin};

VoidServer::new(config)
    .add_plugin(|app| {
        app.add_plugins((NavigationPlugin::new(), NavRemotePlugin::default()));
    })
    .run();
```

## Layout

The crate has three modules. Only `adapter` knows about VoidMC.

| Module | Depends on | Contents |
|---|---|---|
| `pathing` | nothing | Packed voxel cells, a section cache, the walk/fly/swim movement model, a resumable A*, path smoothing and a path follower |
| `adapter` | `voidmc`, Bevy | `NavigationPlugin`, `Navigator`, goals, `Behaviours`, `NavigationEvent`, block-state classification, a chunk-backed cell cache |
| `remote` | `adapter` | `NavRemotePlugin`: the operator wand, `/nav`, and path rendering for the watcher only |

`pathing` builds with `default-features = false` and has no engine
dependency. To port the crate to another engine, or to a redesigned VoidMC
core, rewrite `adapter`. The engine only has to implement one trait:

```rust
pub trait CellSource {
    fn fill_section(&self, section: SectionPos, cells: &mut [Cell; 4096]) -> bool;
}
```

## Navigators and goals

```rust
use voidmc_navigation::{Goal, Navigator, walking_profile};

EntityBuilder::new(EntityKind::Zombie)
    .at(0.5, 64.0, 0.5)
    .gravity(true)
    .block_collision(true)
    .with(
        Navigator::new(walking_profile(EntityKind::Zombie))
            .with_speed(0.2)
            .with_goal(Goal::patrol([[0.5, 64.0, 0.5], [12.5, 64.0, 0.5], [12.5, 64.0, 9.5]])),
    )
    .spawn(&mut commands);
```

`walking_profile(kind)` sizes the body to the collider VoidMC gives that
entity kind. It also uses the collider's step height, so paths match what the
physics can climb.

| Method | Effect |
|---|---|
| `move_to(pos)` / `move_near(pos, radius)` | Walk to a point. Fires `Reached` on arrival. |
| `follow(entity, distance)` | Keep within `distance` of a moving entity. Re-plans when the entity moves away. Never fires `Reached`. |
| `flee(entity, distance)` | Get at least `distance` away from the entity, then fire `Reached`. |
| `patrol(points)` | Loop through the points forever. A point that cannot be reached fires `Unreachable` and is skipped. |
| `set_goal(goal)` | Any of the above as a `Goal` value. Setting the goal already being pursued does nothing. |
| `stop()` | Drop the goal and fire `Interrupted`. |
| `pause()` / `resume()` | Freeze in place without losing the goal. |
| `set_speed(blocks_per_tick)` | Walking speed; the default is `0.15`. |
| `goal()`, `path()`, `waypoint()`, `destination()`, `is_moving()`, `is_idle()` | Read-only state. |

While it has a goal, a navigator owns the entity's horizontal `Velocity` and
yaw. Gravity, block collision and step-up stay with the engine physics.
Navigation systems run in `NavigationSystems`, after the built-in wander AI and
before the physics step. A navigator that is walking a path therefore overrides
`Wander`. While it waits for a path, `Wander` can still move the entity.

`MoveTo` and `Flee` goals end in exactly one `NavigationEvent { entity, outcome }`.
A `Follow` goal fires only `Interrupted`: when no path exists, it waits and retries. A patrol never ends by
itself and can fire `Unreachable` once per skipped point:

| Outcome | When |
|---|---|
| `Reached` | A `MoveTo` or `Flee` goal is satisfied. |
| `Unreachable` | A `MoveTo` or `Flee` search found no path, or the mob stayed stuck for `max_failures` re-plans in a row. When a `Follow` re-plan fails, the mob keeps walking its current path. A patrol fires it for each skipped point and backs off after `max_failures` skips in a row. |
| `Interrupted` | The goal was replaced or stopped, or its target entity despawned or changed dimension. |

```rust
fn on_arrival(event: On<NavigationEvent>, names: Query<&CustomName>) {
    if event.outcome == NavigationOutcome::Reached {
        // ...
    }
}
```

## Behaviours

`Behaviours` is a priority list of goal selectors, re-evaluated every
`interval` ticks (default 10). Entities are staggered so their evaluations
don't all land on the same tick. The first behaviour that doesn't `Pass`
drives the navigator. `Keep` holds the current goal and `Pursue(goal)` sets
a new one.

```rust
use voidmc_navigation::{Behaviour, Behaviours, Choice, Goal};

entity.insert(
    Behaviours::new()
        .with(Behaviour::flee_nearest_player(5.0, 12.0))
        .with(Behaviour::follow_nearest_player(24.0, 3.0))
        .with(Behaviour::wander(home, 10.0)),
);

let guard_post = Behaviour::new("guard_post", move |ctx| {
    if ctx.position.distance(post) > 20.0 {
        Choice::Pursue(Goal::move_to(post))
    } else {
        Choice::Pass
    }
});
```

The built-ins are `follow_nearest_player`, `flee_nearest_player`, `patrol` and
`wander`. A custom behaviour is passed a `BehaviourContext`. It gives the
entity's position, dimension and navigator, whether this behaviour is the
active one, the nearest ready players, and a per-entity `random(salt)`.

## Profiles

`NavigationProfile` is plain data describing the body and its movement rules.

| Field | Walker default | Meaning |
|---|---|---|
| `mobility` | `Walk` | `Walk`, `Fly` or `Swim` (swimmers stay in water) |
| `width`, `height` | `0.6`, `1.8` | Body size. A body wider than one block also checks the columns it overlaps: one ring for up to 3 blocks wide, two rings up to 5. |
| `step_height`, `jump_height` | `0.6`, `1.25` | Rises up to the step height are walked; higher ones are jumped and cost extra. |
| `max_fall` | `3` | The longest drop allowed, in blocks. |
| `water_cost`, `hazard_cost`, `lava_cost` | `Some(4.0)`, `None`, `None` | Cost multiplier for each terrain kind. `None` makes it impassable. |
| `max_nodes`, `nodes_per_block` | `4096`, `32` | Search cap: `nodes_per_block` × the straight-line distance, between 256 and `max_nodes`. A hopeless short search fails fast. |
| `heuristic_weight` | `1.25` | Weighted A*. 1.0 gives optimal paths; higher values expand fewer nodes. |
| `allow_partial` | `true` | When the goal can't be reached, walk to the closest point found instead. |

`NavigationProfile::flyer()` and `::swimmer()` are the other presets. A flying
navigator moves the entity's `Position.y` itself, so spawn the entity with
`.gravity(false)`.

Walker goals snap to the nearest floor in their column, up to 8 blocks up or
down. A point in the air above a hill, or buried in it, still resolves to
somewhere the mob can stand.

### Movement model

The world is a grid of 16-bit cells, one per block. Each cell holds:

- the vertical extent of the collision crossing the middle of the block, in
  sixteenths (up to 24 for fences and walls);
- the sides blocked by thin panels such as doors, panes and trapdoors;
- a terrain kind: normal, water, lava, hazard, stair or unloaded.

A walker stands on any surface with room for its body. It steps or jumps
onto rises within `jump_height` (stairs count as half steps) and drops up to
`max_fall`. It never cuts a corner diagonally, and it crosses a side only if
no panel blocks it. Paths are string-pulled: straight runs across flat, plain
ground collapse into a single waypoint.

### Block models

`NavigationPlugin::new().block_model(..)` chooses how block states map to
cells:

- `BlockModel::FullBlocks` (default) treats every block except air and water
  as a full cube, exactly like VoidMC's current entity physics. Paths then
  match what mobs can actually do.
- `BlockModel::Vanilla` reads the vendored collision shapes, so slabs, stairs,
  carpets, fences, walls, doors and trapdoors keep their real geometry. Switch
  to it once the physics collides with real shapes.

Both models tag water, lava and hazards (fire, campfires, magma, cactus,
sweet berry bushes, wither roses, powder snow, cobwebs).

## Performance

Navigation is budgeted per tick. Searches reuse their buffers, so once the
terrain around the mobs is cached, planning allocates only when the path cache
grows. The first visit to a new area allocates its cell sections.

- **Cell cache.** Sections are converted to cells the first time a search
  reads them: about 2 µs each, palette decoding included. Before each planning
  pass, every cached section of a chunk whose `ChunkData` changed since the
  last pass is decoded again and compared, so any mix of engine block changes,
  direct `ChunkData` edits, bulk section writes and component replacement is
  picked up before the next search. An unloaded chunk is evicted.
- **Shared planner.** Every navigator queues its request in one FIFO.
  `NavigationSettings::expansions_per_tick` (default 2000, about 0.65 ms)
  caps the nodes all searches may expand in one tick. A request whose path is
  cached is served the tick it is made, wherever it sits in the queue. Every
  search starts in the main lane, which spends at most an eighth of the budget
  (at least 32 nodes) on it in total. A search still running past that is
  long: it moves to one of two long lanes, which share a quarter of the budget
  (plus whatever the main lane leaves) and resume where they stopped. When
  both are busy, further long searches wait their turn in order and restart
  from scratch, so every one of them eventually completes or reports
  `Unreachable`. A long search therefore costs the others one eighth of a
  tick's budget, once per request, however large its search limit and however
  many of them run. Searches reuse the open set, the node arena and a dense
  128×32×128 node window (a hash map covers nodes outside it).
- **Path cache.** Paths to fixed goals (`MoveTo`, patrol points) are cached by
  start block, goal block and profile. An entry stays valid until a cached
  cell changes in a chunk within reach of the path: its points widened by the
  body's half width and footprint, so wide bodies are covered too. Digging
  elsewhere does not affect it. Patrols re-plan their legs almost for free.
- **Following** costs about 11 ns per mob per tick.

`NavigationStats` (a resource) reports searches, complete, partial and
unreachable counts, cache hits, nodes expanded, the planning time of the last
tick, the queue length and the number of moving navigators. `/nav info`
prints the same.

Measured on an Apple M-series laptop with `cargo bench -p voidmc-navigation`
(criterion; shared machine, so expect ±15%):

| Scenario | Result |
|---|---|
| 64-block walk on flat ground, warm cache (65 nodes) | 25 µs |
| 96-block walk over hills with trees and slabs (126 nodes) | 55 µs warm, 95 µs cold |
| 254-block diagonal across hills | 104 µs |
| 96-block flight over hills (137 nodes) | 188 µs |
| 164-block maze (22,862 nodes, run uncapped) | 6.6 ms in total, spread over about 12 ticks by the budget |
| Unreachable goal 41 blocks away (distance-scaled cap of 1312 nodes) | 376 µs |
| Section fill (chunk palette to 4096 cells) | 1.8 µs |
| 500 path followers, one tick | 5.4 µs |
| 500 wandering mobs on generated terrain: whole tick, physics included | 322 µs (204 µs of it planning, about 5 searches per tick) |
| 500 patrolling mobs: whole tick, physics included | 217 µs (79 µs of it planning, path-cache hits) |
| The same 500 patrollers while a block changes every tick | 213 µs |
| 1000 wandering mobs / 1000 patrolling mobs | 633 µs / 430 µs |
| Physics alone for 1000 idle mobs, for comparison | 105 µs |

The search core expands about 3,000 to 5,000 nodes per millisecond. A
typical 20-block trip over hills takes about 120 nodes, so roughly 25 to
40 µs.

## Example

`void-example` turns all of this on. The first player to join, or anyone
running `/navdemo`, gets:

- two pigs patrolling a square;
- a wolf that follows the nearest player and wanders when no one is around;
- three chickens that flee anyone who comes within 5 blocks.

The remote is open to everyone there, and the starter kit includes the blaze
rod.

## Remote

`NavRemotePlugin` is the operator layer. By default only players carrying
`Operator` can use it; `.access(RemoteAccess::Everyone)` opens it to everyone.

- **Wand** (a blaze rod by default, `.wand("minecraft:...")` to change it):
  - right-click a mob: select it;
  - right-click a block, near or far: send the mob there;
  - right-click the sky: release the mob.
- **`/nav <action> [x y z]`**:
  - `wand`: give yourself the wand;
  - `select`: the mob you look at, or the nearest one within 16 blocks;
  - `deselect`;
  - `goto x y z`, `here`, `follow`: send the mob somewhere, or bring it to you;
  - `stop`, `pause`, `resume`;
  - `info`: the mob's goal and path, plus the planner stats.

For the selected mob, the watching operator sees:

- the path as thin `block_display` beams: lime for a complete path, orange for
  a partial one. Each beam disappears as the mob passes it.
- a glowing gold marker on the destination.
- a glowing outline on the mob.
- the mob (green) and its destination (orange) on the locator bar, sent with
  the `waypoint` packet.

These are all client-side entities, metadata or waypoints, sent to the
watcher only. The server keeps no entity for them, and other players see
nothing. While nobody holds a `NavRemote`, the render system does not run at
all.

`voidmc_navigation::remote::select(world, operator, Some(mob))` does the same
selection from code.

## Using the core without VoidMC

```rust
use voidmc_navigation::pathing::{
    ArrayWorld, BlockPos, Cell, NavigationProfile, Path, Pathfinder, SearchRequest,
};

let mut world = ArrayWorld::new(BlockPos::new(-32, 0, -32), 64, 16, 64);
world.fill(BlockPos::new(-32, 0, -32), BlockPos::new(31, 0, 31), Cell::FULL);

let mut pathfinder = Pathfinder::new();
let mut path = Path::default();
let status = pathfinder.find(
    &mut world,
    &NavigationProfile::walker(),
    SearchRequest::new([0.5, 1.0, 0.5], [20.5, 1.0, 7.5]),
    &mut path,
);
```

For incremental use, call `Pathfinder::start` once, then `step(world, &mut budget)`
every tick until the status is no longer `Pending`, then `write_path`. To
follow the path, `PathFollower::tick` turns it into a per-tick `Steering`:
velocity, yaw and a jump flag.

## Limits

- VoidMC's physics collides with every non-air block as a full cube, so
  `FullBlocks` is the default model. Shape-accurate paths (`Vanilla`) need a
  shape-accurate physics step first.
- Walkers walk along the bottom of water, as the physics does. There is no
  swimming to the surface.
- There is no hierarchical (chunk-level) planning. A long trip is reached
  through successive partial paths, each capped by `max_nodes`.
- Doors are never opened. A closed door is a wall.
