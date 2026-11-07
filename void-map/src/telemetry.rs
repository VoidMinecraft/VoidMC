//! What a map session went through, to tell when and why it stutters.
//!
//! `RUST_LOG=voidmc_map=debug` logs each event. Every session also logs a summary at
//! `info`, so that a bad stretch shows up without any setup.

use std::time::{Duration, Instant};

/// Past this, a tick came late.
const LATE_TICK: Duration = Duration::from_millis(70);
/// Between two rotation reports of a moving mouse, past this the view went unfed.
const REPORT_HOLE: Duration = Duration::from_millis(120);
/// A longer silence means the mouse was resting.
const MOUSE_RESTED: Duration = Duration::from_secs(1);
/// Past this, sending chunks took a noticeable share of a tick.
const SLOW_SEND: Duration = Duration::from_millis(10);

/// One stretch of a session, between two summaries.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Summary {
    pub ticks: u32,
    pub late_ticks: u32,
    pub longest_tick: Duration,
    pub reports: u32,
    pub report_holes: u32,
    pub longest_hole: Duration,
    pub past_limits: u32,
    pub chunks: usize,
    pub bytes: usize,
    pub longest_send: Duration,
}

#[derive(Debug, Default)]
pub struct Telemetry {
    now: Summary,
    last_tick: Option<Instant>,
    last_report: Option<Instant>,
    past: bool,
}

impl Telemetry {
    pub fn tick(&mut self, at: Instant) {
        self.now.ticks += 1;
        if let Some(last) = self.last_tick.replace(at) {
            let gap = at - last;
            self.now.longest_tick = self.now.longest_tick.max(gap);
            if gap > LATE_TICK {
                self.now.late_ticks += 1;
                tracing::debug!(gap_ms = gap.as_millis() as u64, "map late tick");
            }
        }
    }

    pub fn report(&mut self, at: Instant) {
        self.now.reports += 1;
        if let Some(last) = self.last_report.replace(at) {
            let gap = at - last;
            if gap > REPORT_HOLE && gap < MOUSE_RESTED {
                self.now.report_holes += 1;
                self.now.longest_hole = self.now.longest_hole.max(gap);
                tracing::debug!(gap_ms = gap.as_millis() as u64, "map report hole");
            }
        }
    }

    /// Notes whether the view stands past its limits, counting each time it goes there.
    pub fn past_limits(&mut self, past: bool, yaw: f32, pitch: f32) {
        if past && !self.past {
            self.now.past_limits += 1;
            tracing::debug!(yaw, pitch, "map look past its limits");
        }
        self.past = past;
    }

    pub fn sent(&mut self, chunks: usize, bytes: usize, took: Duration) {
        self.now.chunks += chunks;
        self.now.bytes += bytes;
        self.now.longest_send = self.now.longest_send.max(took);
        if took > SLOW_SEND {
            tracing::debug!(
                chunks,
                took_ms = took.as_millis() as u64,
                "map slow chunk send"
            );
        }
    }

    /// Hands over what happened since the last summary.
    pub fn summary(&mut self) -> Summary {
        std::mem::take(&mut self.now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_late_tick_counts_and_a_regular_one_does_not() {
        #[expect(
            clippy::disallowed_methods,
            reason = "a test sets a real time on purpose"
        )]
        let start = Instant::now();
        let mut telemetry = Telemetry::default();
        telemetry.tick(start);
        telemetry.tick(start + Duration::from_millis(50));
        telemetry.tick(start + Duration::from_millis(150));
        let summary = telemetry.summary();
        assert_eq!(summary.ticks, 3);
        assert_eq!(summary.late_ticks, 1);
        assert_eq!(summary.longest_tick, Duration::from_millis(100));
    }

    #[test]
    fn a_resting_mouse_is_no_hole() {
        #[expect(
            clippy::disallowed_methods,
            reason = "a test sets a real time on purpose"
        )]
        let start = Instant::now();
        let mut telemetry = Telemetry::default();
        telemetry.report(start);
        telemetry.report(start + Duration::from_millis(200));
        telemetry.report(start + Duration::from_secs(5));
        assert_eq!(telemetry.summary().report_holes, 1);
    }

    #[test]
    fn going_past_the_limits_counts_once_until_back() {
        let mut telemetry = Telemetry::default();
        telemetry.past_limits(true, 0.0, 0.0);
        telemetry.past_limits(true, 1.0, 0.0);
        telemetry.past_limits(false, 0.0, 0.0);
        telemetry.past_limits(true, 0.0, 0.0);
        assert_eq!(telemetry.summary().past_limits, 2);
    }

    #[test]
    fn a_summary_starts_a_new_stretch() {
        let mut telemetry = Telemetry::default();
        telemetry.sent(3, 100, Duration::from_millis(1));
        assert_eq!(telemetry.summary().chunks, 3);
        assert_eq!(telemetry.summary().chunks, 0);
    }
}
