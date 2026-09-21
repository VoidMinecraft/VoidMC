# Entities

Server-owned entities (mobs, dropped items, displays) are ECS entities spawned
through `EntityBuilder`. The framework replicates them to the players whose
loaded chunks contain them, streams their movement, metadata and passengers,
and removes them from clients when they despawn.

## Spawning

```rust
use voidmc::{EntityBuilder, EntityKind};

fn spawn_guard(mut commands: Commands, pos: Position, dim: DimensionId) {
    EntityBuilder::new(EntityKind::Zombie)
        .position(pos)            // or .at(x, y, z)
        .in_dimension(dim)        // default Overworld
        .rotation(90.0, 0.0)
        .gravity(true)            // server-side vertical physics
        .block_collision(true)
        .with(Guard)              // extra components declared before the spawn
        .spawn(&mut commands);    // EntityCommands; .spawn_in(&mut world) from a World
}
```

Declare extra components up front with `.with(component)` (or `.with_bundle(bundle)`
for several at once); they land on the entity the moment it spawns, so you never
need a follow-up `.insert(...)`. If you would rather hand a raw bundle straight to
Bevy, `EntityBuilder::bundle()` returns the assembled bundle:
`commands.spawn(EntityBuilder::new(EntityKind::Zombie).at(x, y, z).bundle())`.

`EntityKind` is generated from the `minecraft:entity_type` registry
(`EntityKind::from_name("minecraft:pig")`, `.id()`, `.name()`, `EntityKind::ALL`).
The builder allocates the network id and UUID, derives the collision box from the
kind and fills every component `SpawnedEntity` requires, so an entity is never
half-spawned. `SpawnedEntity` itself cannot be constructed outside `void`; use it
only as a query filter (`With<SpawnedEntity>`).

Other builder methods: `velocity(Velocity)`, `collider(EntityCollider)`,
`movement(MovementConfig)`, `wander(bool)`, `settle_ticks(u8)`.

## Lifecycle and visibility

- A player sees an entity while their `LoadedChunks` contain the entity's chunk
  in the entity's dimension. The tracker (`VoidSystems::EntityVisibility`,
  `PostUpdate`) sends `SpawnEntity` when that becomes true and `RemoveEntities`
  when it stops, and fires `EntityShownEvent { entity, viewer }` /
  `EntityHiddenEvent { entity, network_id, viewer }`.
- Movement, velocity, metadata and passenger changes go only to current
  viewers. `EntityViewers` on the entity lists them.
- Remove an entity with `commands.entity(e).despawn()`; every viewer gets
  `RemoveEntities`, whichever way the entity goes away. `EntityDespawnEvent`
  remains as a trigger-style hook.

Player entities are still replicated dimension-wide (see
[Players](/reference/gameplay/players)).

## Metadata

Every spawned entity carries an `EntityMetadata` component: the indexed synched
data the client renders from. Typed components write into it, and
`VoidSystems::EntityMetadataSync` sends only the indices that changed since the
last tick (`SetEntityData`). A player who starts seeing the entity receives the
full state once.

| Component | Effect |
|---|---|
| `CustomName::new("Bob")` / `.hidden()` | Name tag (indices 2, 3) |
| `Glowing`, `Invisible` | Entity flag bits (index 0) |
| `NoGravity`, `Silent` | Indices 5 and 4 |
| `ItemEntity { stack }` | Rendered item of a `minecraft:item` entity |

Insert to enable, `remove::<T>()` to restore the vanilla default (every shipped
source implements `clear`, including the display components). Mutating a
component's fields re-sends only what changed. These components act on
server-owned entities only; player entities are replicated by the player
systems and ignore them.

For indices without a typed component, write `EntityMetadata` directly:

```rust
use voidmc::EntityMetadata;
use voidmc_protocol::clientbound::EntityMetadataValue;

fn freeze(mut meta: Query<&mut EntityMetadata, With<Frozen>>) {
    for mut meta in &mut meta {
        meta.set(7, EntityMetadataValue::Int(140)); // ticks frozen
    }
}
```

`set` marks the index dirty only when the value changed; `touch(index)` forces a
resend. Your own components get the same treatment by implementing
`MetadataSource` and registering them:

```rust
use voidmc::{EntityMetadata, MetadataSource, MetadataSourceAppExt};

#[derive(Component)]
struct Frozen(i32);

impl MetadataSource for Frozen {
    fn write(&self, meta: &mut EntityMetadata) {
        meta.set(7, EntityMetadataValue::Int(self.0));
    }
    fn clear(meta: &mut EntityMetadata) {
        meta.set(7, EntityMetadataValue::Int(0));
    }
}

app.add_metadata_source::<Frozen>();
```

## Display entities

`BlockDisplay`, `ItemDisplay` and `TextDisplay` require a `Display` component
holding the shared settings (transform, interpolation, billboard, brightness,
view range, shadow, glow color, culling box). Spawn the matching `EntityKind`:

```rust
use voidmc::{BlockDisplay, Display, DisplayTransform, EntityBuilder, EntityKind};
use voidmc_data::v26_1_2::blocks;

EntityBuilder::new(EntityKind::BlockDisplay)
    .position(pos)
    .with(BlockDisplay(blocks::CYAN_STAINED_GLASS))
    .spawn(&mut commands);
```

As above, `.with(...)` attaches the component during the spawn; reach for
`bundle()` (`commands.spawn(builder.bundle())`) when you want the raw bundle.

Assigning `Display::transform` starts a new keyframe that the client
interpolates over `interpolation_ticks`; the interpolation clock reset is
always re-sent, even for an identical transform. `view_range` multiplies the
64-block render distance and `culling_box(width, height)` sets the client's
frustum-culling box — a translated or scaled display left at the default 0
flickers at screen edges.

```rust
display.transform = DisplayTransform::default()
    .translation(x, 0.5, z)
    .uniform_scale(0.5)
    .left_rotation([0.0, half.sin(), 0.0, half.cos()]);
```

`ItemDisplay::new(stack).context(ItemDisplayContext::Ground)` and
`TextDisplay::new("Hi").shadow().alignment(TextAlignment::Left).background_color(argb)`
cover the item and text variants.

## Passengers

```rust
world.entity_mut(pig).insert(Passengers::new([chicken]));
```

`Passengers` on the vehicle sends `SetPassengers` to the vehicle's viewers when
it changes and to players who start seeing the vehicle. A passenger that
despawns is pruned automatically. Passengers and vehicle should sit in the same
chunk so viewers know both.

## Example commands

`void-example` ships `/spawn [entity] [--name <text>] [--glow] [--invisible] [--float]`
and `/display <shield|sign|item|ride|clear> [text]`: the shield is eight
`BlockDisplay` segments orbiting the player through keyframed transforms, the
sign a `TextDisplay`, `ride` a chicken riding a pig through `Passengers`.
