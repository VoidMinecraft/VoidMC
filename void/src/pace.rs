//! Where the time and the chance of the server come from: the wall clock and the
//! thread's generator, or the ticks of a stepped run and a generator with a fixed seed.
//!
//! A stepped server must send the same packets on every run, so nothing it sends may
//! read the wall clock or draw fresh entropy. [`SteppedPacePlugin`] is the one place
//! that switches both, and every reading that ends up in a packet goes through
//! [`Clock`], [`Chance`], [`of`] or [`roll`].
//!
//! Only [`SteppedPacePlugin`] can build a stepped [`Pace`] or a [`Dice`], but like any
//! resource they can still be removed, or replaced by a copy taken from another world.
//! A plugin that does so gives up the guarantee: nothing here prevents it.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bevy_app::{App, First, Plugin};
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::{ExecutorKind, Schedules};
use bevy_ecs::system::SystemParam;
use rand::rngs::StdRng;
use rand::{RngCore, SeedableRng};

/// The seed of every stepped run, so that two runs draw the same numbers.
pub(crate) const STEPPED_SEED: u64 = 0x5EED;

/// The time of the server, and the one count of its ticks. Only [`SteppedPacePlugin`] builds a
/// stepped one; without it, or once it is removed, the server runs on the wall clock.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pace {
    mode: Mode,
    ticks_started: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Real,
    Stepped { tick: Duration },
}

impl Pace {
    pub(crate) const REAL: Pace = Pace {
        mode: Mode::Real,
        ticks_started: 0,
    };

    /// How many ticks have started: 1 during the first. Only a server with a tool that reads it,
    /// the stepped runner or the packet trace, counts them; any other reads 0.
    pub fn ticks_started(&self) -> u64 {
        self.ticks_started
    }

    fn stepped_millis(&self, tick: Duration) -> i64 {
        (tick.as_nanos() * u128::from(self.ticks_started) / 1_000_000) as i64
    }

    /// Milliseconds since the Unix epoch, for dates, or since the first tick of a stepped run.
    /// The wall clock can go back: measure durations with [`Pace::elapsed_millis`].
    pub fn now_millis(&self) -> i64 {
        match self.mode {
            Mode::Real => {
                #[expect(
                    clippy::disallowed_methods,
                    reason = "the one reading of the wall clock that Pace stands for"
                )]
                let now = SystemTime::now();
                now.duration_since(UNIX_EPOCH)
                    .map_or(0, |since| since.as_millis() as i64)
            }
            Mode::Stepped { tick } => self.stepped_millis(tick),
        }
    }

    /// Milliseconds on a clock that never goes back, for durations: since the first reading
    /// of the process, or since the first tick of a stepped run.
    pub fn elapsed_millis(&self) -> i64 {
        match self.mode {
            Mode::Real => {
                static ORIGIN: OnceLock<Instant> = OnceLock::new();
                #[expect(
                    clippy::disallowed_methods,
                    reason = "the one monotonic reading that Pace stands for"
                )]
                let now = Instant::now();
                now.duration_since(*ORIGIN.get_or_init(|| now)).as_millis() as i64
            }
            Mode::Stepped { tick } => self.stepped_millis(tick),
        }
    }

    /// A limit of `budget` from now, which never passes on a stepped server.
    pub fn deadline(&self, budget: Duration) -> Deadline {
        match self.mode {
            Mode::Real => {
                #[expect(
                    clippy::disallowed_methods,
                    reason = "the one deadline on the wall clock that Pace stands for"
                )]
                let now = Instant::now();
                Deadline(Some(now + budget))
            }
            Mode::Stepped { .. } => Deadline(None),
        }
    }
}

/// A limit on the work of a tick, from [`Pace::deadline`].
#[derive(Debug, Clone, Copy)]
pub struct Deadline(Option<Instant>);

impl Deadline {
    /// Whether the time is up: always false on a stepped server.
    pub fn passed(&self) -> bool {
        #[expect(
            clippy::disallowed_methods,
            reason = "a deadline set by Pace on the wall clock is read on it"
        )]
        self.0.is_some_and(|end| Instant::now() >= end)
    }
}

/// The pace of a world: real unless the stepped runner set one.
pub fn of(world: &World) -> Pace {
    world.get_resource::<Pace>().copied().unwrap_or(Pace::REAL)
}

/// The pace of the app, as a system parameter.
#[derive(SystemParam)]
pub struct Clock<'w> {
    pace: Option<Res<'w, Pace>>,
}

impl Clock<'_> {
    pub fn pace(&self) -> Pace {
        self.pace.as_deref().copied().unwrap_or(Pace::REAL)
    }

    pub fn now_millis(&self) -> i64 {
        self.pace().now_millis()
    }

    pub fn elapsed_millis(&self) -> i64 {
        self.pace().elapsed_millis()
    }

    pub fn deadline(&self, budget: Duration) -> Deadline {
        self.pace().deadline(budget)
    }
}

/// The generator of a stepped server, seeded the same on every run. Only
/// [`SteppedPacePlugin`] builds one; without it, or once it is removed, draws take fresh
/// entropy.
#[derive(Resource)]
pub struct Dice(Mutex<StdRng>);

/// Draws from the generator of a world: the thread's one unless the stepped runner set one.
pub fn roll<T>(world: &World, draw: impl FnOnce(&mut dyn RngCore) -> T) -> T {
    match world.get_resource::<Dice>() {
        Some(dice) => draw(&mut *dice.0.lock().unwrap_or_else(|e| e.into_inner())),
        None => draw(&mut fresh()),
    }
}

/// The generator of the app, as a system parameter that holds it alone while it runs.
#[derive(SystemParam)]
pub struct Chance<'w> {
    dice: Option<ResMut<'w, Dice>>,
}

impl Chance<'_> {
    pub fn roll<T>(&mut self, draw: impl FnOnce(&mut dyn RngCore) -> T) -> T {
        match self.dice.as_deref_mut() {
            Some(dice) => draw(dice.0.get_mut().unwrap_or_else(|e| e.into_inner())),
            None => draw(&mut fresh()),
        }
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "the one source of entropy that Dice stands for"
)]
fn fresh() -> rand::rngs::ThreadRng {
    rand::thread_rng()
}

/// Makes the app stepped: its time is its ticks, its chance and its UUIDs have a fixed seed,
/// and its systems run one at a time, in the same order on every run. The
/// stepped runner adds it; nothing else sets a stepped [`Pace`] or a [`Dice`].
pub struct SteppedPacePlugin {
    tick: Duration,
}

impl SteppedPacePlugin {
    pub fn new(tick: Duration) -> Self {
        Self { tick }
    }
}

impl Plugin for SteppedPacePlugin {
    fn build(&self, app: &mut App) {
        crate::uuids::seed_for_steps(STEPPED_SEED);
        app.insert_resource(Pace {
            mode: Mode::Stepped { tick: self.tick },
            ticks_started: 0,
        })
        .insert_resource(Dice(Mutex::new(StdRng::seed_from_u64(STEPPED_SEED))));
        count_ticks(app);
    }

    fn finish(&self, app: &mut App) {
        let mut schedules = app.world_mut().resource_mut::<Schedules>();
        for (_, schedule) in schedules.iter_mut() {
            schedule.set_executor_kind(ExecutorKind::SingleThreaded);
        }
    }
}

#[cfg(test)]
pub(crate) fn make_stepped(world: &mut World) {
    let mut app = App::new();
    app.add_plugins(SteppedPacePlugin::new(Duration::from_millis(50)));
    let from = app.world_mut();
    world.insert_resource(from.remove_resource::<Pace>().unwrap());
    world.insert_resource(from.remove_resource::<Dice>().unwrap());
}

struct TickCountPlugin;

impl Plugin for TickCountPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(First, advance);
    }
}

/// Counts the ticks in [`Pace`], on the wall clock unless the server is stepped. A server
/// without a tool that reads the count pays nothing.
pub(crate) fn count_ticks(app: &mut App) {
    if !app.world().contains_resource::<Pace>() {
        app.insert_resource(Pace::REAL);
    }
    if !app.is_plugin_added::<TickCountPlugin>() {
        app.add_plugins(TickCountPlugin);
    }
}

pub(crate) fn advance(mut pace: ResMut<Pace>) {
    pace.ticks_started += 1;
}

#[cfg(test)]
mod tests {
    use bevy_ecs::system::RunSystemOnce;
    use rand::Rng;

    use super::*;

    fn stepped() -> App {
        let mut app = App::new();
        app.add_plugins(SteppedPacePlugin::new(Duration::from_millis(50)));
        app
    }

    #[test]
    fn a_stepped_pace_counts_ticks_and_has_no_deadline() {
        let mut app = stepped();
        assert_eq!(of(app.world()).now_millis(), 0);
        for _ in 0..3 {
            app.update();
        }
        let pace = of(app.world());
        assert_eq!(pace.now_millis(), 150);
        assert_eq!(pace.elapsed_millis(), 150);
        assert!(!pace.deadline(Duration::ZERO).passed());
    }

    #[test]
    fn each_update_is_one_tick_however_often_the_count_is_asked_for() {
        let mut once = App::new();
        count_ticks(&mut once);
        let mut twice = stepped();
        count_ticks(&mut twice);
        let systems = |app: &App| app.get_schedule(First).map(|first| first.systems_len());
        assert_eq!(systems(&once), systems(&twice));
        for _ in 0..2 {
            once.update();
            twice.update();
        }
        assert_eq!(of(once.world()).ticks_started(), 2);
        assert!(of(once.world()).now_millis() > 1_700_000_000_000);
        assert_eq!(of(twice.world()).ticks_started(), 2);
        assert_eq!(of(twice.world()).now_millis(), 100);
    }

    #[test]
    fn without_a_pace_the_wall_clock_runs() {
        let app = App::new();
        let pace = of(app.world());
        assert_eq!(pace, Pace::REAL);
        assert!(pace.now_millis() > 1_700_000_000_000);
        let first = pace.elapsed_millis();
        std::thread::sleep(Duration::from_millis(20));
        let second = pace.elapsed_millis();
        assert!((0..1_000_000).contains(&first), "{first}");
        assert!(second - first >= 20, "{first} then {second}");
        assert!(pace.deadline(Duration::ZERO).passed());
    }

    #[test]
    fn two_stepped_apps_draw_the_same_numbers() {
        let draws = || {
            let mut app = stepped();
            let first: Vec<u32> = (0..8)
                .map(|_| roll(app.world(), |rng| rng.gen_range(0..1_000_000)))
                .collect();
            let second = app
                .world_mut()
                .run_system_once(|mut chance: Chance| chance.roll(|rng| rng.next_u64()))
                .unwrap();
            (first, second)
        };
        assert_eq!(draws(), draws());
    }

    #[test]
    fn a_stepped_app_runs_its_systems_one_at_a_time() {
        let mut app = stepped();
        app.add_systems(bevy_app::Update, || {});
        app.edit_schedule(bevy_app::Update, |schedule| {
            schedule.set_executor_kind(ExecutorKind::MultiThreaded);
        });
        app.finish();
        let schedules = app.world().resource::<Schedules>();
        let kinds: Vec<ExecutorKind> = schedules
            .iter()
            .map(|(_, schedule)| schedule.get_executor_kind())
            .collect();
        assert!(kinds.len() > 1);
        assert!(
            kinds
                .iter()
                .all(|kind| *kind == ExecutorKind::SingleThreaded),
            "{kinds:?}"
        );
    }
}
