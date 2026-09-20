# Sounds

Sounds are fire-and-forget requests: describe a `Sound`, hand it to `Sounds`
(a `SystemParam`) or `WorldSounds` (from `&World`), and the packets go out
immediately. Nothing is stored on an entity.

## Playing a sound

```rust
use voidmc::{Sound, SoundSource, Sounds};

fn on_pickup(sounds: Sounds, orbs: Query<(&Position, &EntityDimension), With<PickedUp>>) {
    for (position, dimension) in &orbs {
        sounds.play(
            Sound::new("entity.experience_orb.pickup")
                .category(SoundSource::Players)
                .volume(1.0)
                .pitch(1.2)
                .at(position)
                .in_dimension(dimension.0),
        );
    }
}
```

| Builder | Meaning | Default |
|---|---|---|
| `Sound::new(name)` | A `minecraft:sound_event` registry entry (`"entity.player.levelup"` or `"minecraft:entity.player.levelup"`), resolved when played. | required |
| `Sound::custom(id)` | A resource-pack sound sent inline; `.fixed_range(blocks)` sets its audible range. | — |
| `.category(SoundSource)` | Client volume slider: `Master`, `Music`, `Records`, `Weather`, `Blocks`, `Hostile`, `Neutral`, `Players`, `Ambient`, `Voice`, `Ui`. | `Master` |
| `.volume(f32)` | `≥ 0`; above `1.0` also extends the audible range. | `1.0` |
| `.pitch(f32)` | clamped to `0.5..=2.0`. | `1.0` |
| `.seed(i64)` | Picks the variant of multi-sound events. | `0` |
| `.at(position)` + `.in_dimension(dimension)` | Emit from a point (`(f64, f64, f64)` or `&Position`). | — |
| `.from_entity(entity)` | Emit from an entity (follows it client-side). | — |
| `.audience(Audience)` | Who hears it; see below. | see below |

A sound must be placed with `.at(..)` or `.from_entity(..)` (or sent with
`play_to`, below). An unknown registry name or an unplaced sound panics in
debug builds and is logged once and dropped in release builds; `play` returns
whether a packet was sent.

## Audience

The default audience is the players who can see the emitting position: with
`.in_dimension(..)` (or an entity carrying a dimension) that is everyone in the
dimension whose `LoadedChunks` contains the emitter's chunk; without a dimension
it is every ready player. Any [`Audience`](../server/sending-packets.md#audiences)
overrides it.

```rust
use voidmc::{Audience, DimensionId, Sound};

Sound::new("ambient.cave").at((0.0, 30.0, 0.0)).audience(Audience::InDimension(DimensionId::Overworld));
```

`play_to(player, sound)` targets one player: the audience is that player alone
and an unplaced sound is emitted at their own position. Use it for UI feedback.

```rust
sounds.play_to(player, Sound::custom("myserver:ui/ding").category(SoundSource::Ui));
```

## Stopping sounds

`SoundStop` selects by category and/or sound name (both unset stops everything)
and takes an `Audience` (default: every ready player).

```rust
use voidmc::{SoundSource, SoundStop};

sounds.stop(SoundStop::all().source(SoundSource::Music));
sounds.stop(SoundStop::all().sound("myserver:ui/ding").audience(Audience::explicit([player])));
```

## From exclusive-world code

`WorldSounds::new(world)` offers the same `play`, `play_to` and `stop`, for
command handlers and observers that hold a `&World`. The example server's
`/sound <id> [volume] [pitch]` command uses it (`void-example/src/sound.rs`).
