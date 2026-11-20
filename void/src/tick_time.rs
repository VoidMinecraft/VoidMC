//! The time the game thread spends working on a tick, from the start of
//! `First` to the end of `Last`: the scheduler's wait between ticks is left
//! out. Measured once here and read by the slow-tick warning
//! (`metrics_debug`) and the observability endpoints, whichever are enabled.

use std::time::{Duration, Instant};

use bevy_app::{App, First, Last, MainScheduleOrder, Plugin};
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ScheduleLabel;

/// Runs before `First`, so that a tick is measured whole.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TickStart;

/// Runs after `Last`. Systems that read [`TickWorkTime`] run here, after
/// [`TickMeasured`].
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TickEnd;

/// The system of [`TickEnd`] that stops the clock.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TickMeasured;

/// The work time of the tick that just ended, up to date in [`TickEnd`] after
/// [`TickMeasured`].
#[derive(Resource, Debug)]
pub(crate) struct TickWorkTime {
    started: Option<Instant>,
    /// From the start of `First` to the end of `Last`.
    pub(crate) last: Duration,
    /// When `Last` ended.
    pub(crate) ended_at: Instant,
}

/// Measures [`TickWorkTime`]. Added through [`require`], so that the plugins
/// that read it share one measurement.
pub(crate) struct TickTimePlugin;

impl Plugin for TickTimePlugin {
    fn build(&self, app: &mut App) {
        {
            let mut order = app.world_mut().resource_mut::<MainScheduleOrder>();
            order.insert_before(First, TickStart);
            order.insert_after(Last, TickEnd);
        }
        app.insert_resource(TickWorkTime {
            started: None,
            last: Duration::ZERO,
            ended_at: Instant::now(),
        })
        .add_systems(TickStart, start_tick)
        .add_systems(TickEnd, end_tick.in_set(TickMeasured));
    }
}

/// Adds [`TickTimePlugin`] unless an earlier plugin did.
pub(crate) fn require(app: &mut App) {
    if !app.is_plugin_added::<TickTimePlugin>() {
        app.add_plugins(TickTimePlugin);
    }
}

fn start_tick(mut work: ResMut<TickWorkTime>) {
    work.started = Some(Instant::now());
}

fn end_tick(mut work: ResMut<TickWorkTime>) {
    let now = Instant::now();
    work.last = work
        .started
        .take()
        .map_or(Duration::ZERO, |started| now - started);
    work.ended_at = now;
}

#[cfg(test)]
mod tests {
    use bevy_app::Update;

    use super::*;

    #[test]
    fn measures_the_work_of_a_tick_and_not_the_wait_between_ticks() {
        let mut app = App::new();
        require(&mut app);
        require(&mut app);
        app.add_systems(Update, |mut ticks: Local<u32>| {
            *ticks += 1;
            if *ticks == 1 {
                std::thread::sleep(Duration::from_millis(20));
            }
        });

        app.update();
        let first = app.world().resource::<TickWorkTime>().last;
        assert!(first >= Duration::from_millis(20), "{first:?}");

        std::thread::sleep(Duration::from_millis(100));
        app.update();
        let second = app.world().resource::<TickWorkTime>().last;
        assert!(second < Duration::from_millis(100), "{second:?}");
    }
}
