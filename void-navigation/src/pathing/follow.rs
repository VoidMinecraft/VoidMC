use super::math::{Vec3, yaw_towards};
use super::path::Path;
use super::profile::{Mobility, MoveModel};

const CORNER_REACH: f64 = 0.35;
const FINAL_REACH: f64 = 0.2;
const PROGRESS_EPSILON: f64 = 0.05;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Steering {
    pub velocity: Vec3,
    pub yaw: Option<f32>,
    pub jump: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FollowStatus {
    Moving(Steering),
    Arrived,
    Stuck,
}

/// Turns a [`Path`] into per-tick steering: aims at the current waypoint,
/// advances when it is reached, asks for a jump on rises above the step
/// height, and reports `Stuck` when no progress is made for `stuck_ticks`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathFollower {
    index: usize,
    best_distance: f64,
    idle_ticks: u16,
    stuck_ticks: u16,
}

impl Default for PathFollower {
    fn default() -> Self {
        Self::new(40)
    }
}

impl PathFollower {
    pub fn new(stuck_ticks: u16) -> Self {
        Self {
            index: 0,
            best_distance: f64::INFINITY,
            idle_ticks: 0,
            stuck_ticks: stuck_ticks.max(1),
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new(self.stuck_ticks);
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn tick(
        &mut self,
        path: &Path,
        position: Vec3,
        grounded: bool,
        speed: f64,
        model: &MoveModel,
    ) -> FollowStatus {
        let points = path.points();
        let free = model.mobility != Mobility::Walk;
        loop {
            let Some(&target) = points.get(self.index) else {
                return FollowStatus::Arrived;
            };
            let last = self.index + 1 == points.len();
            let delta = target - position;
            let reach = if last {
                FINAL_REACH.max(speed * 0.5)
            } else {
                CORNER_REACH.max(speed)
            };
            let reached = if free {
                delta.length() <= reach
            } else {
                delta.horizontal_length() <= reach && delta.y.abs() < 1.0
            };
            if !reached {
                break;
            }
            self.index += 1;
            self.best_distance = f64::INFINITY;
            self.idle_ticks = 0;
        }

        let target = points[self.index];
        let delta = target - position;
        let distance = if free {
            delta.length()
        } else {
            delta.horizontal_length()
        };
        if distance < self.best_distance - PROGRESS_EPSILON {
            self.best_distance = distance;
            self.idle_ticks = 0;
        } else {
            self.idle_ticks += 1;
            if self.idle_ticks >= self.stuck_ticks {
                return FollowStatus::Stuck;
            }
        }

        let pace = speed.min(distance);
        let velocity = if free {
            delta.normalize_or_zero() * pace
        } else {
            Vec3::new(delta.x, 0.0, delta.z).normalize_or_zero() * pace
        };
        let jump = !free
            && grounded
            && delta.y > model.step as f64 / 16.0 + 1.0e-3
            && delta.horizontal_length() < 1.5;
        let yaw =
            (velocity.x != 0.0 || velocity.z != 0.0).then(|| yaw_towards(velocity.x, velocity.z));
        FollowStatus::Moving(Steering {
            velocity,
            yaw,
            jump,
        })
    }
}
