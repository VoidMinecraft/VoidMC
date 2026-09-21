# World Border

A world border is an entity carrying a `WorldBorder` component. Spawn it to
show it, mutate it to update it, despawn it to remove it. The framework tracks
what each viewer has been sent and emits only the packet for the field that
changed.

The border is **client-side only**: it draws the wall, tints the screen inside
the warning distance and lets the client block movement through it, but the
engine never pushes players back, damages them or validates their position
against it. Gameplay rules that depend on the border belong in your own
systems.

## Showing a border

```rust
use std::time::Duration;

use voidmc::{Audience, WorldBorder};

fn start_round(mut commands: Commands) {
    commands.spawn(
        WorldBorder::new()
            .center(0.0, 0.0)
            .diameter(512.0)
            .warning_blocks(8)
            .warning_time(Duration::from_secs(15))
            .audience(Audience::All),
    );
}
```

| Field | Type | Default |
|---|---|---|
| `center_x`, `center_z` | `f64` | `0.0`, `0.0` |
| diameter | `f64` in `1.0..=59_999_968.0` | `59_999_968.0` (vanilla) |
| `warning_blocks` | `u32` | `5` |
| `warning_time` | `Duration` | 15 s |
| `portal_teleport_boundary` | `u32` | `29_999_984` (vanilla) |
| `audience` | [`Audience`](../server/sending-packets.md#audiences) | `Audience::All` |

The builder methods (`.center()`, `.diameter()`, `.warning_blocks()`,
`.warning_time()`, `.portal_teleport_boundary()`, `.audience()`,
`.viewers([..])`) set the same fields. Diameters are clamped to the vanilla
range (NaN becomes the default). Durations are sent to the client in ticks
(50 ms); anything finer is truncated.

## Updating a border

Mutate the component like any other; each changed aspect becomes one packet in
the next `PostUpdate` (`VoidSystems::WorldBorderSync`). Unchanged fields send
nothing.

| Change | Packet |
|---|---|
| `set_center(x, z)` / assigning `center_x`, `center_z` | Set Border Center |
| `set_diameter(d)` | Set Border Size |
| `shrink_to(d, duration)` | Set Border Lerp Size |
| `warning_blocks` | Set Border Warning Distance |
| `warning_time` | Set Border Warning Delay |
| `portal_teleport_boundary` | Initialize World Border (no dedicated packet exists) |

```rust
fn shrink_arena(mut borders: Query<&mut WorldBorder, With<Arena>>) {
    for mut border in &mut borders {
        border.shrink_to(64.0, Duration::from_secs(120));
    }
}
```

`shrink_to` animates from the diameter viewers currently see to the target
over `duration`; it works for growing too. The component keeps the target and
the deadline, so a player who joins the audience mid-way receives an
Initialize World Border with the interpolated current diameter and the
remaining time. `current_diameter()`, `target_diameter()` and
`is_transitioning()` read that state back; calling `set_diameter` or a new
`shrink_to` cancels the running transition.

## Audience

`audience` decides who sees the border. With the default `Audience::All`, every
ready player sees it: players joining later receive the full state on their
first ready tick. Any other `Audience` narrows it; changing the audience sends
the full state to newcomers and resets players who no longer qualify to the
vanilla border.

One border per player at a time: if several `WorldBorder` entities include the
same player, the client keeps whichever packet arrived last. Use
`Audience::InDimension` for one border per dimension:

```rust
use voidmc::{Audience, DimensionId, WorldBorder};

fn nether_border(mut commands: Commands) {
    commands.spawn(
        WorldBorder::new()
            .diameter(256.0)
            .audience(Audience::InDimension(DimensionId::Nether)),
    );
}

fn personal_border(mut commands: Commands, player: Entity) {
    commands.spawn(WorldBorder::new().diameter(32.0).viewers([player]));
}
```

## Removing a border

Despawn the entity, or remove the `WorldBorder` component; viewers immediately
receive an Initialize World Border carrying the vanilla defaults (centre `0,0`,
diameter `59_999_968`, warning distance `5`, warning time 15 s).

The example server registers `/border [diameter] [seconds] [--remove]` as a
minimal demonstration (`void-example/src/world_border.rs`).
