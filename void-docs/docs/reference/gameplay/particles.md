# Particles

Particles are fire-and-forget requests: build one with `Particles::spawn`
(a `SystemParam`) or `WorldParticles::new(&world).spawn(..)` (from command
handlers, item behaviours and other `&World` code), chain the settings you
need, and finish with `.send()`. The `LevelParticles` packet goes to every
ready player that has the position's chunk loaded in the request's dimension,
unless you set an `Audience`. A request that is never sent does nothing (the
builder is `#[must_use]`).

```rust
use voidmc::{Particle, ParticleColor, Particles, DimensionId};
use voidmc::players::Audience;

fn on_block_break(event: On<BlockBreakEvent>, particles: Particles) {
    particles
        .spawn(Particle::Block { state: event.broken_state })
        .at_block(event.position)
        .dimension(event.dimension)
        .count(12)
        .offset(0.3, 0.3, 0.3)
        .speed(0.05)
        .send();
}

fn heal_burst(particles: Particles, healed: Query<(Entity, &Position), Added<Healed>>) {
    for (entity, position) in &healed {
        particles
            .spawn(Particle::Dust { color: ParticleColor::rgb(0x2E, 0xE0, 0x6F), scale: 1.5 })
            .at(position)
            .count(30)
            .offset(0.5, 1.0, 0.5)
            .audience(Audience::InDimension(DimensionId::Overworld))
            .send();
    }
}
```

## Request settings

| Method | Default | Meaning |
|---|---|---|
| `at(pos)` / `at_block(BlockPosition)` | `[0, 0, 0]` | Emit position (`&Position`, `Position`, `[f64; 3]`). |
| `dimension(DimensionId)` | `Overworld` | Dimension used for the default audience. |
| `count(n)` | `1` | Number of particles. |
| `offset(x, y, z)` | `0, 0, 0` | Random spread on each axis. |
| `speed(s)` | `0.0` | Maximum speed. |
| `directed(dir, speed)` | — | Directed mode (`count = 0`): every particle moves along `dir` × `speed`. |
| `long_distance(bool)` | `false` | Ignore the client's particle distance limit. |
| `always_visible(bool)` | `false` | Render even when the client's particle setting is "minimal". |
| `audience(Audience)` / `viewers(entities)` | chunk viewers | Replace the default audience. |
| `send()` | — | Consumes the request and sends it. |

`packet()` and `recipients()` return what `send()` would send and to whom, for tests.

## Particle types

`voidmc::Particle` has one variant per `minecraft:particle_type` of 26.1.2
(117). Types that carry a payload carry it in the variant, so a payload can't
be forgotten:

| Variants | Payload |
|---|---|
| `Block`, `BlockMarker`, `FallingDust`, `DustPillar`, `BlockCrumble` | `state: i32` — a block-state id (`voidmc_data::v26_1_2::blocks::*` are default states). |
| `Dust` | `color: ParticleColor`, `scale: f32` (client clamps to 0.01..4.0). |
| `DustColorTransition` | `from`, `to: ParticleColor`, `scale: f32`. |
| `EntityEffect`, `TintedLeaves`, `Flash` | `color: ParticleColor` (ARGB). |
| `Effect`, `InstantEffect` | `color: ParticleColor` (`ParticleColor(-1)` = default), `power: f32`. |
| `DragonBreath` | `power: f32`. |
| `Item` | `stack: ItemStackTemplate` (`ItemStackTemplate::try_from(&item_stack)`, `Err` for an empty stack, or `::simple(item_id, count)`). |
| `Vibration` | `source: PositionSource::{Block, Entity}`, `arrival_in_ticks: i32`. |
| `Trail` | `target: [f64; 3]`, `color`, `duration: i32`. |
| `SculkCharge` | `roll: f32`. |
| `Shriek` | `delay: i32`. |
| everything else (`Flame`, `Smoke`, `Heart`, …) | none. |

`ParticleColor::rgb(r, g, b)` / `::argb(a, r, g, b)` build colours.
`Particle::simple("flame")` resolves a payload-free type by name (with or
without `minecraft:`), `Particle::needs_data(name)` tells whether a name is a
payload type, and `Particle::NAMES` lists every registry name. Ids are
resolved from `voidmc_data` at encode time, never hardcoded.

## Example command

`void-example` ships `/particle <type> [count]`: it resolves the name with
`Particle::simple`, replies with an error for unknown or payload-carrying
names, and emits the particle one block above the player through
`WorldParticles`.
