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
| `division` | `BossBarDivision` (`Progress`, `Notches6`, `Notches10`, `Notches12`, `Notches20`) | `Progress` |
| `flags` | `BossBarFlags` (`DARKEN_SCREEN`, `BOSS_MUSIC`, `WORLD_FOG`) | empty |
| `audience` | [`Audience`](../server/sending-packets.md#audiences) | `Audience::All` |

The builder methods (`.title()`, `.progress()`, `.color()`, `.division()`,
`.flags()`, `.darken_screen()`, `.boss_music()`, `.world_fog()`, `.audience()`,
`.viewers([..])`) set the same fields. `progress` is clamped to `0.0..=1.0` on
the wire whether you go through `set_progress` or assign the field (NaN counts
as `0.0`).

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

## Audience

`audience` decides who sees the bar. With the default `Audience::All`, every
ready player sees it: players joining later receive it on their first ready
tick, players leaving are forgotten. Any other `Audience` narrows it; changing
the audience shows the bar to newcomers and removes it from players who no
longer qualify on the next tick.

```rust
use voidmc::{Audience, BossBar, DimensionId};

fn personal_timer(mut commands: Commands, player: Entity) {
    commands.spawn(BossBar::new("Time left").viewers([player]));
}

fn nether_warning(mut commands: Commands) {
    commands.spawn(BossBar::new("The Nether").audience(Audience::InDimension(DimensionId::Nether)));
}

fn invite(mut bars: Query<&mut BossBar, With<Party>>, player: Entity) {
    for mut bar in &mut bars {
        if let Audience::Explicit(members) = &mut bar.audience {
            members.insert(player);
        }
    }
}
```

## Removing a bar

Despawn the entity, or remove the `BossBar` component; viewers receive the
remove action immediately.

The example server registers `/bossbar [progress] [--remove]` as a minimal
demonstration (`void-example/src/boss_bar.rs`).
