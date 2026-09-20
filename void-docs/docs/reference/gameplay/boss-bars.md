# Boss Bars

A boss bar is an entity carrying a `BossBar` component. Spawn it to show it,
mutate its fields to update it, despawn it to remove it. The framework tracks
what each player has been sent and emits only the matching Boss Event action.

## Showing a bar

```rust
use voidmc::{BossBar, BossBarColor, BossBarDivision};

fn start_raid(mut commands: Commands) {
    commands.spawn(
        BossBar::new("Raid")
            .color(BossBarColor::Red)
            .division(BossBarDivision::Notches10)
            .progress(1.0)
            .darken_screen(),
    );
}
```

| Field | Type | Default |
|---|---|---|
| `title` | `String` | required |
| `progress` | `f32` in `0.0..=1.0` | `1.0` |
| `color` | `BossBarColor` (`Pink`, `Blue`, `Red`, `Green`, `Yellow`, `Purple`, `White`) | `Pink` |
| `division` | `BossBarDivision` (`None`, `Notches6`, `Notches10`, `Notches12`, `Notches20`) | `None` |
| `flags` | `BossBarFlags` (`DARKEN_SCREEN`, `BOSS_MUSIC`, `WORLD_FOG`) | empty |

The builder methods (`.title()`, `.progress()`, `.color()`, `.division()`,
`.flags()`, `.darken_screen()`, `.boss_music()`, `.world_fog()`) set the same
fields; `set_progress` clamps to the valid range.

## Updating a bar

Mutate the component like any other; each changed aspect becomes one update
packet in the next `PostUpdate` (`VoidSystems::BossBarSync`). Unchanged fields
send nothing.

```rust
fn tick_raid(mut bars: Query<&mut BossBar, With<Raid>>, raid: Res<RaidState>) {
    for mut bar in &mut bars {
        bar.set_progress(raid.remaining as f32 / raid.total as f32);
        if raid.remaining == 0 {
            bar.title = "Raid cleared".into();
            bar.color = BossBarColor::Green;
        }
    }
}
```

## Viewers

By default every ready player sees every bar: players joining later receive it
on their first ready tick, players leaving are forgotten. Add a
`BossBarViewers` component to restrict the audience to an explicit set of player
entities; `add` / `remove` on it show or hide the bar for that player on the
next tick.

```rust
use voidmc::{BossBar, BossBarViewers};

fn personal_timer(mut commands: Commands, player: Entity) {
    commands.spawn((
        BossBar::new("Time left").progress(1.0),
        BossBarViewers::new([player]),
    ));
}
```

## Removing a bar

Despawn the entity, or remove the `BossBar` component; viewers receive the
remove action immediately.

The example server registers `/bossbar [progress] [--remove]` as a minimal
demonstration (`void-example/src/boss_bar.rs`).
