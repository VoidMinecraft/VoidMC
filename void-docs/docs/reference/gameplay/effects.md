# Status Effects & Attributes

Both are stateful, per-entity components on the same model as [Boss Bars](./boss-bars.md):
insert the component, mutate it, and a `PostUpdate` system
(`VoidSystems::EntityMetadataSync`) diffs it against what each client was sent
and pushes only the change. Removing the component, or despawning the
entity, cleans the clients up. They work on players and on entities spawned
with `EntityBuilder` alike.

Effect and attribute ids are the generated `Effect` and `EntityAttribute`
enums (`voidmc::Effect::Speed`, `voidmc::EntityAttribute::MovementSpeed`), backed by
the `minecraft:mob_effect` and `minecraft:attribute` registries of the data
crate; `.name()`, `.id()`, `Effect::from_name("minecraft:speed")` and
`EntityAttribute::ALL` are available. `Effect::color()` / `category()` /
`particle()` and `EntityAttribute::default_value()` / `range()` /
`is_client_syncable()` carry the vanilla tables, and
`EntityKind::default_attribute(attribute)` / `EntityKind::is_living()` the
per-kind `DefaultAttributes` supplier.

## Status effects

```rust
use voidmc::{Effect, EffectDuration, EffectInstance, StatusEffects};

commands.entity(player).insert(
    StatusEffects::new()
        .with(Effect::Speed, 1, EffectDuration::seconds(30))
        .with_instance(
            Effect::Glowing,
            EffectInstance::new(0, EffectDuration::Infinite).hide_particles(),
        ),
);
```

`with(effect, amplifier, duration)` shows particles and the HUD icon; use
`with_instance` with an `EffectInstance` for the other flags:

| `EffectInstance` | Default | Builder |
|---|---|---|
| `amplifier: u8` | required | `EffectInstance::new(amplifier, duration)` |
| `duration: EffectDuration` | required | `EffectDuration::ticks(n)`, `::seconds(n)`, `::Infinite` |
| `ambient: bool` | `false` | `.ambient()` — beacon-style faint particles |
| `show_particles: bool` | `true` | `.hide_particles()` |
| `show_icon: bool` | `true` | `.hide_icon()`; `.hidden()` turns both off |

On a live component:

```rust
fn boost(mut effects: Query<&mut StatusEffects, With<Boosting>>) {
    for mut effects in &mut effects {
        effects.add(Effect::Speed, 2, EffectDuration::ticks(60)).hide_icon();
        effects.get_mut(Effect::Glowing).map(|mut g| g.amplifier = 1);
        effects.remove(Effect::Slowness);
    }
}
```

`add` replaces an existing instance of the same effect and returns a slot with
the same builder methods (`.ambient()`, `.hide_particles()`, `.hide_icon()`,
`.hidden()`) plus `DerefMut` to the `EffectInstance`. `get_mut` marks the effect
dirty; `get`, `has`, `iter`, `len`, `is_empty` read; `remove` and `clear`
drop effects.

### Expiry

Durations are server-authoritative. `EffectDuration::Ticks(n)` counts down
every tick from the tick after it was sent; at zero the effect is removed from
the component and a `RemoveMobEffect` goes out. `Infinite` stays until removed.
Re-adding an effect restarts its duration and sends one `UpdateMobEffect`.

### Who receives what

The routing follows vanilla:

- **The entity's own client** (a player) gets `UpdateMobEffect` /
  `RemoveMobEffect` for every change — that is what drives the HUD icons and
  client-side movement. A player who is not ready yet gets the full list on the
  first ready tick.
- **Player passengers** of the entity get the same packets for the vehicle
  (vanilla shows a ridden mob's effects on its rider's HUD); dismounting sends
  the removals.
- **Other viewers** never get effect packets. Visible effects reach them
  through entity metadata: `show_particles` adds the effect's particle (its
  vanilla colour, or the custom particle of `trial_omen`, `raid_omen`,
  `wind_charged`, `weaving`, `oozing`, `infested`) to the living-entity
  particle list and the ambience flag (`true` when every effect that shows
  particles is ambient), and `Effect::Glowing` / `Effect::Invisibility` set the
  entity flags exactly like the `Glowing` / `Invisible` marker components do.
  Players have no `EntityMetadata`, so a player's effects are not shown to
  other players.
- The particle list and ambience flag are `LivingEntity` metadata: they are
  only projected for players and kinds with `EntityKind::is_living()`; a
  `StatusEffects` on a boat, item or display entity logs a warning and only
  the glowing / invisible flags apply.
- The `blend` flag (smooth fog / darkness transition) is set on the entity's
  own client when an effect is first added, never on passengers or updates,
  as vanilla does.

Only the changed effect is sent: bumping an amplifier is one packet, expiry is
one removal, an unchanged component sends nothing.

## Attributes

```rust
use voidmc::{Attributes, EntityAttribute, Modifier, ModifierOperation};

commands.entity(player).insert(
    Attributes::new()
        .base(EntityAttribute::MovementSpeed, 0.1)
        .modifier(
            EntityAttribute::MovementSpeed,
            Modifier::new("voidmc:kart_boost", 0.5, ModifierOperation::AddMultipliedTotal),
        )
        .base(EntityAttribute::Scale, 2.0),
);
```

An attribute that is never touched keeps its vanilla default; `base(..)`
overrides it, `modifier(..)` stacks a modifier on top (on the vanilla default
base if none was set). Modifier ids are resource locations and unique per
attribute: adding one with an existing id replaces it.

### Defaults are per entity kind

The default is the one vanilla's `AttributeSupplier` gives that *kind*
(`EntityKind::default_attribute`), not the attribute registry's: a player's
`MovementSpeed` is `0.1`, a zombie's `0.23`, a horse's `0.225`, while the
registry default is `0.7`. A client keeps the last snapshot it received, so
this is the base every reset must send back.

- `Attributes::new()` (and `Default`) assumes a **player**
  (`Attributes::for_player()`).
- `Attributes::for_kind(kind)` seeds from that kind; only needed to read
  `value()` before the component is inserted, because inserting any
  `Attributes` on an entity spawned with `EntityBuilder` — the first time or
  as a replacement of the current component — re-seeds it to the entity's
  kind: bases you set explicitly are kept, bases that were still the default
  move to the kind's default. `kind()` and `default_base(attribute)` read
  them back.
- Replacing the component also resets, on every client that had them, the
  attributes the previous component synced and the new one does not carry.

```rust
let mut speed = Attributes::new();
speed.add_modifier(EntityAttribute::MovementSpeed, Modifier::multiply_total("demo:boost", 1.0));
speed.value(EntityAttribute::MovementSpeed); // 0.2 on a player, 0.46 once inserted on a zombie
```

Vanilla's `0.23F`-style float literals are widened to `f64` the way Java does
(`0.2300000041723251`), so the bytes match a Paper server.

| `Modifier` | Wire operation | Effect on the value |
|---|---|---|
| `Modifier::add(id, amount)` | `AddValue` | `base + amount` |
| `Modifier::multiply_base(id, amount)` | `AddMultipliedBase` | `+ (base after additions) × amount` |
| `Modifier::multiply_total(id, amount)` | `AddMultipliedTotal` | `× (1 + amount)`, applied last |

`Modifier::new(id, amount, operation)` takes the operation explicitly.

On a live component: `set_base`, `add_modifier`, `remove_modifier(attribute,
id)`, `clear_modifiers`, `reset(attribute)` (drops every override and sends the
kind's default back), and the readers `get`, `has`, `iter`, `value(attribute)`
— the final number the client computes, clamped to the attribute's range.

### Who receives what

Each tick with changes sends one `UpdateAttributes` holding only the changed
attributes (full snapshot of each: base plus every modifier). Following
vanilla, only attributes with `is_client_syncable()` cross the wire — movement
speed, scale, gravity, jump strength, max health, block/entity interaction
range, ... — the eight server-only ones (attack damage, attack knockback,
follow range, knockback resistance, spawn reinforcements, tempt range,
waypoint ranges) stay in the component for your own logic and `value()`.

- **The entity's own client** and **every viewer** (the `EntityViewers` of a
  tracked entity; every other ready player for a player) get the same diff.
- A viewer that starts seeing a tracked entity, and a player joining while
  another player has attributes, receive the full syncable set.
- A player who stops seeing another player, `reset(attribute)`, and removing
  the component all send the kind's vanilla default so the client does not
  keep a stale value.

## Example commands

`void-example` ships `/effect <name|clear> [seconds] [amplifier] [--hidden]
[--ambient]` (`seconds` 0 removes that effect, omitted means infinite) and
`/speed <factor>` (a `multiply_total` modifier on the player's vanilla
`0.1` movement speed; `1.0` removes it).
