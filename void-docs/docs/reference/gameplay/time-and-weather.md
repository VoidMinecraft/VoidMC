# Time & Weather

Time of day and weather are entities carrying a `WorldTime` or a `Weather`
component. Spawn one to start syncing it, mutate its fields to change it,
despawn it to stop. The framework tracks what each viewer has been sent and
pushes only what changed; nothing is recomputed per tick beyond the clock
itself.

Both follow the [`BossBar`](./boss-bars.md) pattern: an `audience` field
picks the viewers, a required `*State` component holds the sync bookkeeping,
and a `PostUpdate` set (`VoidSystems::WorldTimeSync`, `VoidSystems::WeatherSync`)
does the sending.

## World time

```rust
use voidmc::{WorldTime, WorldClock};

fn setup(mut commands: Commands) {
    commands.spawn(WorldTime::new().time_of_day(6000));
    commands.spawn(WorldTime::new().time_of_day(18000).frozen().viewers([vip]));
}
```

| Field | Type | Default |
|---|---|---|
| `time_of_day` | `i64` — unbounded, like vanilla; `day_time()` gives `0..24000` | `0` |
| `frozen` | `bool` — pauses `time_of_day` | `false` |
| `clock` | `WorldClock` (`Overworld`, `End`) — which synced world clock the packet addresses | `Overworld` |
| `audience` | [`Audience`](../server/sending-packets.md#audiences) | `Audience::All` |

Builders: `.time_of_day()`, `.frozen()`, `.clock()`, `.audience()`,
`.viewers([..])`. Mutators: `set(ticks)`, `add(ticks)`, `freeze()`,
`resume()`, `is_frozen()`, `day_time()`. Assigning the fields directly works
too.

The game time of every packet comes from the `GameTime` resource — one
server-wide tick counter the plugin advances every tick — so a client moving
between clocks (a personal override over the world clock) keeps one monotonic
timeline for animation phases. Read it with `Res<GameTime>`; setting it is
allowed but not detected as a change, the next Set Time packet carries it.

### What gets sent

The server clock advances `time_of_day` by one each tick unless frozen. The
natural advance bypasses change detection, so `Changed<WorldTime>` only fires
on your own edits.

A Set Time packet (game time + one `ClockNetworkState` for `clock`: total
ticks, partial tick `0`, rate `1.0` running / `0.0` frozen) goes out:

- to every viewer on the first tick, on `set`/`add` (any value the clock did
  not reach by itself), on `freeze`/`resume`, or on a `clock` change;
- to every viewer every `TIME_SYNC_INTERVAL` (20) ticks while the clock runs,
  like vanilla — the client extrapolates in between. A frozen clock sends
  nothing until something changes;
- to a viewer joining the audience, immediately.

A viewer leaving the audience of a frozen clock, or losing it to a despawn or
component removal, is sent the same time with rate `1.0` so the client clock
runs again. Leaving a running clock sends nothing.

`WorldClock::registry_id()` resolves `minecraft:overworld` / `minecraft:the_end`
through the `minecraft:world_clock` registry of `voidmc_data`, not through
the `RegistryDataStore` actually sent to clients: if you edit the store's
`minecraft:world_clock` entries the holder ids no longer match, as with biomes.
Overworld-type dimensions read the overworld clock, the End reads its own.

## Weather

```rust
use voidmc::{Weather, WeatherKind, Audience, DimensionId};

fn storm(mut commands: Commands) {
    commands.spawn(Weather::thunder().transition(100).audience(Audience::InDimension(DimensionId::Overworld)));
}

fn calm(mut weather: Query<&mut Weather>) {
    for mut w in &mut weather {
        w.set_clear();
    }
}
```

| Field | Type | Default |
|---|---|---|
| `kind` | `WeatherKind` — `Clear`, `Rain { level }`, `Thunder { rain, thunder }` | `Clear` |
| `transition` | `u32` — ticks over which rain and thunder levels move to their target; `0` jumps | `0` |
| `audience` | [`Audience`](../server/sending-packets.md#audiences) | `Audience::All` |

Constructors: `Weather::clear()`, `Weather::rain()` (level `1.0`),
`Weather::thunder()` (both `1.0`), `Weather::new(kind)`. Builders: `.kind()`,
`.transition()`, `.audience()`, `.viewers([..])`. Mutators: `set(kind)`,
`set_clear()`, `set_rain()`, `set_thunder()`, `is_raining()`. Levels are
clamped to `0.0..=1.0` on the wire (NaN counts as `0.0`).
`WeatherState::levels()` reports the current on-wire `(rain, thunder)`,
mid-transition included.

### What gets sent

Weather is Game Event packets: Begin Raining (1) / End Raining (2) when the
raining flag (`kind != Clear`) flips, Rain Level Change (7) and Thunder Level
Change (8) with the float level. Per tick, viewers receive only the events
whose value changed; on a flag flip both levels are resent, as vanilla does.
With a `transition`, each level moves linearly from where it was to its target
over exactly `transition` ticks (an elapsed-tick counter, no float drift) and
each step is one packet, so a vanilla-like fade of 100 ticks costs exactly 100
small packets per viewer, the last one landing on the target. Changing `kind`
mid-transition restarts the ramp from the current on-wire levels.

A viewer joining the audience always receives the full state — the flag event
and both levels, even for clear weather — so it lands on this component's
state whatever it was shown before. A viewer leaving the audience, or losing
the component to a despawn or removal, is reset to clear (End Raining, both
levels `0`) when the last synced state was not already clear.

## Precedence: explicit viewers win

Several components may cover the same player: a per-dimension clock plus a
personal one with `.viewers([player])`, world rain plus a personal clear sky.
A component with an explicit audience (`Audience::Explicit`, i.e.
`.viewers([..])`) claims its ready members: every non-explicit component
(`All`, `InDimension`, `Custom`) leaves them out of its viewers for as long as
the override covers them. A running world clock therefore never touches a
player who has a personal frozen clock, and changing or ramping the world
weather never reaches a player with a personal sky. Spawn order does not
matter.

- On hand-over to an override the world-level component sends nothing; the
  override sends its full state.
- When the override goes away (despawn, removal, the player leaving its
  explicit set or going not-ready), it sends its usual reset (rate `1.0` for
  a frozen clock, End + levels `0` for weather), and the world-level component
  sees the player as newly joined and re-sends its full state, on the same
  tick when the audience changed and on the next tick after a despawn. No
  resync code on your side.
- Among components of the same kind — two `All` clocks, or two explicit
  overrides over the same player — the client keeps whatever it received
  last (query order), and a component the player leaves makes the others
  covering them re-send their full state on the next tick.

## Example commands

The example server spawns one `WorldTime` and one `Weather` for everyone and
registers `/time <set|add|freeze|resume|query> [day|noon|night|midnight|ticks]`
and `/weather <clear|rain|thunder> [transition]`
(`void-example/src/environment.rs`).
