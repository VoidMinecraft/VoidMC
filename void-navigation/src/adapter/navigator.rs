use bevy_ecs::prelude::*;

use crate::pathing::{MoveModel, NavigationProfile, Path, PathFollower, Vec3};

pub const DEFAULT_SPEED: f64 = 0.15;

/// What a [`Navigator`] is trying to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Goal {
    MoveTo { target: Vec3, radius: f64 },
    Follow { target: Entity, distance: f64 },
    Flee { from: Entity, distance: f64 },
    Patrol { points: Vec<Vec3>, next: usize },
}

impl Goal {
    pub fn move_to(target: impl Into<Vec3>) -> Self {
        Goal::MoveTo {
            target: target.into(),
            radius: 0.0,
        }
    }

    pub fn follow(target: Entity, distance: f64) -> Self {
        Goal::Follow {
            target,
            distance: distance.max(0.0),
        }
    }

    pub fn flee(from: Entity, distance: f64) -> Self {
        Goal::Flee {
            from,
            distance: distance.max(1.0),
        }
    }

    pub fn patrol(points: impl IntoIterator<Item = impl Into<Vec3>>) -> Self {
        Goal::Patrol {
            points: points.into_iter().map(Into::into).collect(),
            next: 0,
        }
    }

    fn same_intent(&self, other: &Goal) -> bool {
        match (self, other) {
            (Goal::Patrol { points: a, .. }, Goal::Patrol { points: b, .. }) => a == b,
            _ => self == other,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavigationOutcome {
    Reached,
    Unreachable,
    Interrupted,
}

/// Fired when a navigator's goal ends: reached, given up as unreachable, or
/// interrupted (replaced, stopped, or its target entity is gone). Follow and
/// patrol goals never fire `Reached`; patrols fire `Unreachable` for a point
/// they skip.
#[derive(Event, Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavigationEvent {
    pub entity: Entity,
    pub outcome: NavigationOutcome,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Phase {
    #[default]
    Idle,
    Following,
    Holding,
    Waiting(u16),
}

/// Drives a server-owned entity along computed paths. Insert it on any
/// [`voidmc::components::SpawnedEntity`] and give it a goal:
///
/// ```no_run
/// # use bevy_ecs::prelude::*;
/// # use voidmc_navigation::{Navigator, NavigationProfile};
/// # fn system(mut navigators: Query<&mut Navigator>, player: Entity) {
/// for mut navigator in &mut navigators {
///     navigator.follow(player, 3.0);
/// }
/// # }
/// ```
///
/// The navigator owns the entity's horizontal velocity while it has a goal;
/// gravity and block collision stay with the engine physics.
#[derive(Component, Debug)]
pub struct Navigator {
    profile: NavigationProfile,
    pub(crate) model: MoveModel,
    pub(crate) fingerprint: u64,
    speed: f64,
    goal: Option<Goal>,
    paused: bool,
    pub(crate) path: Path,
    pub(crate) follower: PathFollower,
    pub(crate) phase: Phase,
    pub(crate) revision: u32,
    pub(crate) search_ticket: u32,
    pub(crate) wants_path: bool,
    pub(crate) queued: bool,
    pub(crate) failures: u8,
    pub(crate) repath_cooldown: u16,
    pub(crate) planned_for: Option<Vec3>,
    pub(crate) origin: Vec3,
    pub(crate) outcomes: Vec<NavigationOutcome>,
    pub(crate) owns_velocity: bool,
}

impl Default for Navigator {
    fn default() -> Self {
        let body = voidmc::components::EntityCollider::default();
        Self::new(
            NavigationProfile::walker()
                .size(body.half_width * 2.0, body.height)
                .step_height(body.step_height),
        )
    }
}

impl Navigator {
    pub fn new(profile: NavigationProfile) -> Self {
        let model = profile.compile();
        Self {
            fingerprint: model.fingerprint(),
            model,
            profile,
            speed: DEFAULT_SPEED,
            goal: None,
            paused: false,
            path: Path::default(),
            follower: PathFollower::default(),
            phase: Phase::Idle,
            revision: 0,
            search_ticket: 0,
            wants_path: false,
            queued: false,
            failures: 0,
            repath_cooldown: 0,
            planned_for: None,
            origin: Vec3::ZERO,
            outcomes: Vec::new(),
            owns_velocity: false,
        }
    }

    pub fn with_speed(mut self, speed: f64) -> Self {
        self.speed = speed.max(0.0);
        self
    }

    pub fn with_goal(mut self, goal: Goal) -> Self {
        self.set_goal(goal);
        self
    }

    pub fn speed(&self) -> f64 {
        self.speed
    }

    pub fn set_speed(&mut self, speed: f64) {
        self.speed = speed.max(0.0);
    }

    pub fn profile(&self) -> &NavigationProfile {
        &self.profile
    }

    pub fn set_profile(&mut self, profile: NavigationProfile) {
        self.model = profile.compile();
        self.fingerprint = self.model.fingerprint();
        self.profile = profile;
        self.replan();
    }

    pub fn move_to(&mut self, target: impl Into<Vec3>) {
        self.set_goal(Goal::move_to(target));
    }

    pub fn move_near(&mut self, target: impl Into<Vec3>, radius: f64) {
        self.set_goal(Goal::MoveTo {
            target: target.into(),
            radius: radius.max(0.0),
        });
    }

    pub fn follow(&mut self, target: Entity, distance: f64) {
        self.set_goal(Goal::follow(target, distance));
    }

    pub fn flee(&mut self, from: Entity, distance: f64) {
        self.set_goal(Goal::flee(from, distance));
    }

    pub fn patrol(&mut self, points: impl IntoIterator<Item = impl Into<Vec3>>) {
        self.set_goal(Goal::patrol(points));
    }

    /// Replaces the goal. Setting the goal already being pursued is a no-op,
    /// so behaviours can re-assert theirs every evaluation.
    pub fn set_goal(&mut self, goal: Goal) {
        if self
            .goal
            .as_ref()
            .is_some_and(|current| current.same_intent(&goal))
        {
            return;
        }
        if self.goal.is_some() {
            self.outcomes.push(NavigationOutcome::Interrupted);
        }
        self.goal = Some(goal);
        self.failures = 0;
        self.clear_path();
        self.phase = Phase::Idle;
        self.replan();
    }

    pub fn stop(&mut self) {
        if self.goal.take().is_some() {
            self.outcomes.push(NavigationOutcome::Interrupted);
        }
        self.clear_path();
        self.phase = Phase::Idle;
        self.wants_path = false;
        self.search_ticket = self.search_ticket.wrapping_add(1);
    }

    pub fn pause(&mut self) {
        self.paused = true;
    }

    pub fn resume(&mut self) {
        if self.paused {
            self.paused = false;
            self.replan();
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn goal(&self) -> Option<&Goal> {
        self.goal.as_ref()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The index in [`Self::path`] of the waypoint being walked to.
    pub fn waypoint(&self) -> usize {
        self.follower.index()
    }

    /// Where the entity stood when the current path was planned.
    pub fn path_origin(&self) -> Vec3 {
        self.origin
    }

    /// Bumped every time the path is replaced or cleared.
    pub fn path_revision(&self) -> u32 {
        self.revision
    }

    pub fn is_moving(&self) -> bool {
        !self.paused && self.phase == Phase::Following
    }

    pub fn is_idle(&self) -> bool {
        self.goal.is_none()
    }

    pub fn destination(&self) -> Option<Vec3> {
        match &self.goal {
            Some(Goal::MoveTo { target, .. }) => Some(*target),
            Some(Goal::Patrol { points, next }) => points.get(*next).copied(),
            _ => self.path.end(),
        }
    }

    pub(crate) fn goal_mut(&mut self) -> Option<&mut Goal> {
        self.goal.as_mut()
    }

    pub(crate) fn finish(&mut self, outcome: NavigationOutcome) {
        self.goal = None;
        self.outcomes.push(outcome);
        self.clear_path();
        self.phase = Phase::Idle;
        self.wants_path = false;
        self.search_ticket = self.search_ticket.wrapping_add(1);
    }

    pub(crate) fn replan(&mut self) {
        self.search_ticket = self.search_ticket.wrapping_add(1);
        self.repath_cooldown = 0;
        self.wants_path = self.goal.is_some();
        if self.goal.is_none() {
            self.phase = Phase::Idle;
        }
    }

    pub(crate) fn request_path(&mut self) {
        if self.goal.is_some() {
            self.wants_path = true;
        }
    }

    pub(crate) fn clear_path(&mut self) {
        if !self.path.is_empty() {
            self.path.clear();
            self.revision = self.revision.wrapping_add(1);
        }
        self.follower.reset();
        self.planned_for = None;
    }

    pub(crate) fn path_changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.follower.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacing_a_goal_interrupts_the_previous_one() {
        let mut navigator = Navigator::default();
        navigator.move_to([1.0, 64.0, 1.0]);
        assert!(navigator.outcomes.is_empty());
        navigator.move_to([5.0, 64.0, 1.0]);
        assert_eq!(navigator.outcomes, vec![NavigationOutcome::Interrupted]);
        navigator.stop();
        assert_eq!(navigator.outcomes.len(), 2);
        assert!(navigator.is_idle());
    }

    #[test]
    fn reasserting_the_same_goal_keeps_progress() {
        let mut navigator = Navigator::default();
        navigator.patrol([[0.0, 64.0, 0.0], [8.0, 64.0, 0.0]]);
        if let Some(Goal::Patrol { next, .. }) = navigator.goal_mut() {
            *next = 1;
        }
        navigator.phase = Phase::Following;
        navigator.patrol([[0.0, 64.0, 0.0], [8.0, 64.0, 0.0]]);
        assert_eq!(navigator.phase, Phase::Following);
        assert_eq!(navigator.destination(), Some(Vec3::new(8.0, 64.0, 0.0)));
        assert!(navigator.outcomes.is_empty());
    }

    #[test]
    fn pause_and_resume_replan() {
        let mut navigator = Navigator::default().with_goal(Goal::move_to([3.0, 64.0, 0.0]));
        navigator.phase = Phase::Following;
        navigator.pause();
        assert!(!navigator.is_moving());
        navigator.wants_path = false;
        navigator.resume();
        assert!(navigator.wants_path);
    }
}
