//! How a panning view catches up with where the mouse has put it.
//!
//! Four ways to compare, each from the cameras of games. None changes how far the mouse
//! goes: twice the mouse is twice the distance, at any speed. They differ in what the
//! view does between the mouse and the screen, where a client glides over three ticks.
//!
//! - [`Motion::Damped`] is a critically damped spring, as in Unity's `SmoothDamp`: it
//!   starts and ends on a curve and can never pass the mouse.
//! - [`Motion::Slack`] is a dead zone: the view hangs on a short slack, so that small
//!   shakes and the start of a turn back do nothing. A slow mouse still pulls it along.
//! - [`Motion::Lead`] looks ahead by the speed of the mouse, to make up for the delay
//!   of the client's glide, like the look ahead of a platformer's camera.
//! - [`Motion::Predicted`] tracks position and speed, as a network game tracks a far
//!   entity: a tick with no report goes on at the speed known, and a tick with two is
//!   spread out, so that the steps stay even whatever the reports do.

/// How the mouse drives a pan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Motion {
    Damped,
    Slack,
    Lead,
    /// The shortest delay among those that end nearly where the mouse does.
    #[default]
    Predicted,
}

impl Motion {
    pub const ALL: [Self; 4] = [Self::Damped, Self::Slack, Self::Lead, Self::Predicted];

    pub fn label(self) -> &'static str {
        match self {
            Self::Damped => "A - amorti",
            Self::Slack => "B - zone morte",
            Self::Lead => "C - anticipation",
            Self::Predicted => "D - prédiction",
        }
    }

    /// Whether the view can end up past the mouse, which then stays there.
    pub fn runs_past(self) -> bool {
        matches!(self, Self::Lead | Self::Predicted)
    }

    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|one| *one == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }
}

/// How long a damped view takes to catch up, in ticks.
const DAMP_TICKS: f64 = 1.5;
/// The slack of a dead zone, in blocks.
const SLACK: f64 = 1.5;
/// How much of the way past the slack the view covers in a tick.
const SLACK_SHARE: f64 = 0.55;
/// How far ahead of the mouse a lead looks, in ticks of its speed. It makes up for the
/// glide of the client and for its own step.
const LEAD_TICKS: f64 = 2.5;
const LEAD_SHARE: f64 = 0.5;
/// How much of a new speed the lead takes on in a tick.
const LEAD_SPEED_SHARE: f64 = 0.35;
/// How a prediction takes in what it did not expect, in position and in speed: the gains
/// of a critically damped tracker.
const PREDICT_ALPHA: f64 = 0.64;
const PREDICT_BETA: f64 = 0.16;
/// Ticks with no report before the mouse counts as stopped. Under this, a gap is only
/// the reports falling unevenly on the ticks.
const STOPPED_AFTER: u32 = 2;
/// A stopping view keeps this much of its step each tick, and goes this much of the way
/// to the mouse. A client gliding to a view that stops dead overshoots.
const STOP_KEEP: f64 = 0.45;
const STOP_SHARE: f64 = 0.45;
/// Under this a stopping step is over, in blocks.
const STEP_OVER: f64 = 0.02;

/// What a view remembers from tick to tick, on its way to the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Follower {
    /// The speed of a spring or of a tracker, in blocks per tick.
    speed: [f64; 2],
    /// The speed of the mouse, on average.
    mouse: [f64; 2],
    /// Ticks since the mouse moved.
    idle: u32,
}

impl Follower {
    /// Where the view goes on this tick, from `view`, its last `step`, where the mouse has
    /// put it and how far the mouse went since the last tick.
    pub fn next(
        &mut self,
        motion: Motion,
        view: [f64; 2],
        step: [f64; 2],
        aim: [f64; 2],
        moved: [f64; 2],
    ) -> [f64; 2] {
        self.idle = if moved == [0.0; 2] { self.idle + 1 } else { 0 };
        let next = match motion {
            Motion::Damped => self.damped(view, aim),
            Motion::Slack => slack(view, aim),
            Motion::Lead => self.lead(view, step, aim, moved),
            Motion::Predicted => self.predicted(view, step, aim),
        };
        // A view ahead of the mouse waits for it: only the mouse turns it back.
        std::array::from_fn(|axis| {
            let back = (next[axis] - view[axis]) * step[axis] < 0.0;
            if back && moved[axis] * step[axis] >= 0.0 {
                view[axis]
            } else {
                next[axis]
            }
        })
    }

    /// Whether the mouse has stopped.
    pub fn stopped(&self) -> bool {
        self.idle >= STOPPED_AFTER
    }

    /// `SmoothDamp`, which cannot pass what it follows.
    fn damped(&mut self, view: [f64; 2], aim: [f64; 2]) -> [f64; 2] {
        let omega = 2.0 / DAMP_TICKS;
        let decay = 1.0 / (1.0 + omega + 0.48 * omega.powi(2) + 0.235 * omega.powi(3));
        std::array::from_fn(|axis| {
            let off = view[axis] - aim[axis];
            let push = self.speed[axis] + omega * off;
            self.speed[axis] = (self.speed[axis] - omega * push) * decay;
            let next = aim[axis] + (off + push) * decay;
            if (aim[axis] - view[axis]) * (next - aim[axis]) > 0.0 {
                self.speed[axis] = 0.0;
                aim[axis]
            } else {
                next
            }
        })
    }

    fn lead(&mut self, view: [f64; 2], step: [f64; 2], aim: [f64; 2], moved: [f64; 2]) -> [f64; 2] {
        if self.stopped() {
            self.mouse = [0.0; 2];
            return settle(view, step, aim);
        }
        if self.idle > 0 {
            return [view[0] + step[0], view[1] + step[1]];
        }
        std::array::from_fn(|axis| {
            self.mouse[axis] += (moved[axis] - self.mouse[axis]) * LEAD_SPEED_SHARE;
            let ahead = aim[axis] + self.mouse[axis] * LEAD_TICKS;
            view[axis] + (ahead - view[axis]) * LEAD_SHARE
        })
    }

    fn predicted(&mut self, view: [f64; 2], step: [f64; 2], aim: [f64; 2]) -> [f64; 2] {
        if self.stopped() {
            self.speed = [0.0; 2];
            return settle(view, step, aim);
        }
        let gap = self.idle > 0;
        std::array::from_fn(|axis| {
            let expected = view[axis] + self.speed[axis];
            if gap {
                return expected;
            }
            let surprise = aim[axis] - expected;
            self.speed[axis] += PREDICT_BETA * surprise;
            expected + PREDICT_ALPHA * surprise
        })
    }
}

/// Follows past the slack, and not at all inside it.
fn slack(view: [f64; 2], aim: [f64; 2]) -> [f64; 2] {
    let off = [aim[0] - view[0], aim[1] - view[1]];
    let distance = off[0].hypot(off[1]);
    if distance <= SLACK {
        return view;
    }
    let share = (distance - SLACK) * SLACK_SHARE / distance;
    [view[0] + off[0] * share, view[1] + off[1] * share]
}

/// Ends a move on a tail of shorter and shorter steps, which never turns back: past the
/// mouse, the view stays where it ends up.
fn settle(view: [f64; 2], step: [f64; 2], aim: [f64; 2]) -> [f64; 2] {
    std::array::from_fn(|axis| {
        let pull = (aim[axis] - view[axis]) * STOP_SHARE;
        let keep = step[axis] * STOP_KEEP;
        let moved = if pull * keep > 0.0 && pull.abs() > keep.abs() {
            pull
        } else if keep.abs() < STEP_OVER {
            0.0
        } else {
            keep
        };
        view[axis] + moved
    })
}
