use std::collections::VecDeque;
use std::time::Instant;

use bevy_ecs::lifecycle::Remove;
use bevy_ecs::prelude::*;
use voidmc::components::{
    EntityDimension, Grounded, PlayerDimension, Position, Rotation, Velocity, VerticalVelocity,
};
use voidmc::world::DimensionId;

use super::navigator::{Goal, NavigationEvent, NavigationOutcome, Navigator, Phase};
use super::world::{ChunkCells, NavigationWorld};
use crate::pathing::hash::FastMap;
use crate::pathing::{
    BlockPos, FollowStatus, Mobility, Path, Pathfinder, SearchRequest, SearchStatus, Vec3,
};

const JUMP_VELOCITY: f64 = 0.42;
const RETRY_TICKS: u16 = 40;
const HOLD_SLACK: f64 = 1.0;
const FLEE_MARGIN: f64 = 2.0;
const PATH_CACHE_CAPACITY: usize = 2048;

/// Tunables shared by every navigator.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct NavigationSettings {
    /// Node expansions all searches may spend per tick; a search that runs out
    /// resumes next tick where it stopped.
    pub expansions_per_tick: u32,
    /// Minimum ticks between two re-plans of a moving target.
    pub repath_interval: u16,
    /// Consecutive stuck/unreachable attempts before a goal is abandoned.
    pub max_failures: u8,
}

impl Default for NavigationSettings {
    fn default() -> Self {
        Self {
            expansions_per_tick: 2_000,
            repath_interval: 10,
            max_failures: 3,
        }
    }
}

/// Live counters, refreshed every tick.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct NavigationStats {
    pub searches: u64,
    pub complete: u64,
    pub partial: u64,
    pub unreachable: u64,
    pub cache_hits: u64,
    pub expanded: u64,
    pub expanded_last_tick: u32,
    pub planning_micros_last_tick: u64,
    pub queued: usize,
    pub moving: usize,
}

#[derive(Clone, Copy)]
struct ActiveSearch {
    entity: Entity,
    ticket: u32,
    dimension: DimensionId,
    origin: Vec3,
    key: Option<PathKey>,
    epoch: u64,
}

/// Identifies a search whose result can be reused: same body, same start
/// block, same fixed goal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct PathKey {
    dimension: DimensionId,
    start: BlockPos,
    goal: BlockPos,
    radius: u64,
    model: u64,
}

struct CachedPath {
    path: Path,
    epoch: u64,
}

/// The single shared pathfinder and its FIFO of navigators waiting for a path.
#[derive(Resource, Default)]
pub struct PathPlanner {
    pathfinder: Pathfinder,
    queue: VecDeque<Entity>,
    active: Option<ActiveSearch>,
    cache: FastMap<PathKey, CachedPath>,
}

impl PathPlanner {
    pub fn queued(&self) -> usize {
        self.queue.len() + usize::from(self.active.is_some())
    }
}

type Bodies<'w, 's> = Query<
    'w,
    's,
    (
        &'static Position,
        Option<&'static EntityDimension>,
        Option<&'static PlayerDimension>,
    ),
>;

fn locate(bodies: &Bodies, entity: Entity) -> Option<(Vec3, DimensionId)> {
    let (position, entity_dimension, player_dimension) = bodies.get(entity).ok()?;
    let dimension = entity_dimension
        .map(|d| d.0)
        .or(player_dimension.map(|d| d.0))
        .unwrap_or(DimensionId::Overworld);
    Some((to_vec(position), dimension))
}

pub(crate) fn to_vec(position: &Position) -> Vec3 {
    Vec3::new(position.x, position.y, position.z)
}

pub(crate) fn update_goals(
    settings: Res<NavigationSettings>,
    bodies: Bodies,
    mut navigators: Query<(Entity, &mut Navigator)>,
) {
    for (entity, mut navigator) in &mut navigators {
        if navigator.goal().is_none() || navigator.is_paused() {
            continue;
        }
        let navigator = navigator.as_mut();
        if navigator.repath_cooldown > 0 {
            navigator.repath_cooldown -= 1;
        }
        if let Phase::Waiting(ticks) = navigator.phase {
            if ticks <= 1 {
                navigator.phase = Phase::Idle;
                navigator.request_path();
            } else {
                navigator.phase = Phase::Waiting(ticks - 1);
            }
            continue;
        }
        let Some((me, dimension)) = locate(&bodies, entity) else {
            continue;
        };
        let target = match navigator.goal() {
            Some(Goal::Follow { target, distance }) => Some((*target, *distance, false)),
            Some(Goal::Flee { from, distance }) => Some((*from, *distance, true)),
            _ => None,
        };
        let Some((other, distance, flee)) = target else {
            continue;
        };
        let Some((there, there_dimension)) = locate(&bodies, other) else {
            navigator.finish(NavigationOutcome::Interrupted);
            continue;
        };
        if there_dimension != dimension {
            navigator.finish(NavigationOutcome::Interrupted);
            continue;
        }
        let gap = me.horizontal_distance(there);
        if flee {
            if gap >= distance {
                navigator.finish(NavigationOutcome::Reached);
            } else if navigator.phase == Phase::Idle {
                navigator.request_path();
            } else if moved(navigator.planned_for, there, 2.0) && navigator.repath_cooldown == 0 {
                navigator.request_path();
                navigator.repath_cooldown = settings.repath_interval;
            }
            continue;
        }
        let close = gap <= distance.max(0.5) && (me.y - there.y).abs() < 2.0;
        match navigator.phase {
            Phase::Holding if gap > distance + HOLD_SLACK => navigator.request_path(),
            Phase::Following if close => {
                navigator.clear_path();
                navigator.phase = Phase::Holding;
                navigator.wants_path = false;
            }
            Phase::Following
                if navigator.repath_cooldown == 0
                    && moved(navigator.planned_for, there, (distance * 0.5).max(1.5)) =>
            {
                navigator.request_path();
                navigator.repath_cooldown = settings.repath_interval;
            }
            Phase::Idle if !close => navigator.request_path(),
            Phase::Idle => navigator.phase = Phase::Holding,
            _ => {}
        }
    }
}

fn moved(planned_for: Option<Vec3>, now: Vec3, threshold: f64) -> bool {
    planned_for.is_none_or(|then| then.distance(now) > threshold)
}

pub(crate) fn schedule_searches(
    mut planner: ResMut<PathPlanner>,
    mut navigators: Query<(Entity, &mut Navigator)>,
) {
    for (entity, mut navigator) in &mut navigators {
        if navigator.wants_path && !navigator.queued && !navigator.is_paused() {
            navigator.queued = true;
            planner.queue.push_back(entity);
        }
    }
}

struct Plan {
    request: SearchRequest,
    planned_for: Vec3,
    fixed: bool,
}

fn request_for(navigator: &Navigator, me: Vec3, bodies: &Bodies) -> Option<Plan> {
    let plan = |request, planned_for, fixed| {
        Some(Plan {
            request,
            planned_for,
            fixed,
        })
    };
    match navigator.goal()? {
        Goal::MoveTo { target, radius } => plan(
            SearchRequest::new(me, *target).within(*radius),
            *target,
            true,
        ),
        Goal::Patrol { points, next } => {
            let point = *points.get(*next)?;
            plan(SearchRequest::new(me, point), point, true)
        }
        Goal::Follow { target, distance } => {
            let (there, _) = locate(bodies, *target)?;
            plan(
                SearchRequest::new(me, there).within(*distance),
                there,
                false,
            )
        }
        Goal::Flee { from, distance } => {
            let (threat, _) = locate(bodies, *from)?;
            let mut away = Vec3::new(me.x - threat.x, 0.0, me.z - threat.z);
            if away.horizontal_length() < 1.0e-3 {
                away = Vec3::new(1.0, 0.0, 0.0);
            }
            let reach = (distance - me.horizontal_distance(threat)).max(0.0) + FLEE_MARGIN;
            let point = me + away.normalize_or_zero() * reach;
            plan(SearchRequest::new(me, point).within(1.0), threat, false)
        }
    }
}

pub(crate) fn run_planner(
    settings: Res<NavigationSettings>,
    mut planner: ResMut<PathPlanner>,
    mut world: ResMut<NavigationWorld>,
    mut stats: ResMut<NavigationStats>,
    chunks: ChunkCells,
    bodies: Bodies,
    mut navigators: Query<&mut Navigator>,
) {
    let started = Instant::now();
    let planner = planner.as_mut();
    let mut budget = settings.expansions_per_tick;
    stats.expanded_last_tick = 0;
    while budget > 0 {
        let active = match planner.active {
            Some(active) => active,
            None => {
                let Some(entity) = planner.queue.pop_front() else {
                    break;
                };
                let Ok(mut navigator) = navigators.get_mut(entity) else {
                    continue;
                };
                navigator.queued = false;
                if !navigator.wants_path || navigator.is_paused() {
                    continue;
                }
                let Some((me, dimension)) = locate(&bodies, entity) else {
                    continue;
                };
                let Some(plan) = request_for(&navigator, me, &bodies) else {
                    navigator.finish(NavigationOutcome::Interrupted);
                    continue;
                };
                navigator.wants_path = false;
                navigator.planned_for = Some(plan.planned_for);
                let epoch = world.epoch(dimension);
                let key = plan.fixed.then(|| PathKey {
                    dimension,
                    start: BlockPos::containing(me),
                    goal: BlockPos::containing(plan.request.goal),
                    radius: plan.request.radius.to_bits(),
                    model: navigator.fingerprint,
                });
                if let Some(hit) = key.and_then(|key| planner.cache.get(&key))
                    && hit.epoch == epoch
                {
                    let navigator = navigator.as_mut();
                    navigator.path.copy_from(&hit.path);
                    navigator.origin = me;
                    navigator.path_changed();
                    navigator.phase = Phase::Following;
                    stats.cache_hits += 1;
                    continue;
                }
                navigator.queued = true;
                let request = plan.request;
                let model = navigator.model;
                planner
                    .pathfinder
                    .start(&mut world.view(dimension, &chunks), &model, request);
                stats.searches += 1;
                let active = ActiveSearch {
                    entity,
                    ticket: navigator.search_ticket,
                    dimension,
                    origin: me,
                    key,
                    epoch,
                };
                planner.active = Some(active);
                active
            }
        };
        let before = budget;
        let status = planner
            .pathfinder
            .step(&mut world.view(active.dimension, &chunks), &mut budget);
        stats.expanded_last_tick += before - budget;
        stats.expanded += (before - budget) as u64;
        if status == SearchStatus::Pending {
            break;
        }
        planner.active = None;
        let Ok(mut navigator) = navigators.get_mut(active.entity) else {
            continue;
        };
        navigator.queued = false;
        if navigator.search_ticket != active.ticket {
            continue;
        }
        let navigator = navigator.as_mut();
        if status.found_path() {
            planner.pathfinder.write_path(
                &mut world.view(active.dimension, &chunks),
                &mut navigator.path,
            );
            navigator.origin = active.origin;
            navigator.path_changed();
        }
        let progress = match (status, navigator.path.end(), locate(&bodies, active.entity)) {
            (SearchStatus::Partial, Some(end), Some((me, _))) => end.horizontal_distance(me) > 1.0,
            (status, _, _) => status == SearchStatus::Complete,
        };
        if progress {
            if status == SearchStatus::Complete {
                stats.complete += 1;
                if let Some(key) = active.key {
                    if planner.cache.len() >= PATH_CACHE_CAPACITY {
                        planner.cache.clear();
                    }
                    planner.cache.insert(
                        key,
                        CachedPath {
                            path: navigator.path.clone(),
                            epoch: active.epoch,
                        },
                    );
                }
            } else {
                stats.partial += 1;
            }
            navigator.phase = Phase::Following;
        } else {
            stats.unreachable += 1;
            unreachable(&settings, navigator);
        }
    }
    stats.queued = planner.queued();
    stats.planning_micros_last_tick = started.elapsed().as_micros() as u64;
}

fn unreachable(settings: &NavigationSettings, navigator: &mut Navigator) {
    navigator.clear_path();
    navigator.failures = navigator.failures.saturating_add(1);
    match navigator.goal_mut() {
        Some(Goal::Patrol { points, next }) => {
            if points.len() > 1 {
                *next = (*next + 1) % points.len();
                navigator.outcomes.push(NavigationOutcome::Unreachable);
            }
            if navigator.failures >= settings.max_failures {
                navigator.failures = 0;
                navigator.phase = Phase::Waiting(RETRY_TICKS);
            } else {
                navigator.request_path();
            }
        }
        Some(Goal::Follow { .. }) => navigator.phase = Phase::Waiting(RETRY_TICKS),
        _ => navigator.finish(NavigationOutcome::Unreachable),
    }
}

pub(crate) fn follow_paths(
    settings: Res<NavigationSettings>,
    mut stats: ResMut<NavigationStats>,
    mut navigators: Query<(
        &mut Navigator,
        &mut Position,
        &mut Velocity,
        &mut Rotation,
        &mut VerticalVelocity,
        &mut Grounded,
    )>,
) {
    let mut moving = 0;
    for (mut navigator, mut position, mut velocity, mut rotation, mut vertical, mut grounded) in
        &mut navigators
    {
        let navigator = navigator.as_mut();
        if navigator.is_paused() || navigator.phase != Phase::Following {
            if navigator.owns_velocity {
                navigator.owns_velocity = false;
                halt(&mut velocity);
            }
            continue;
        }
        moving += 1;
        let model = navigator.model;
        let status = navigator.follower.tick(
            &navigator.path,
            to_vec(&position),
            grounded.0,
            navigator.speed(),
            &model,
        );
        match status {
            FollowStatus::Moving(steering) => {
                navigator.owns_velocity = true;
                if velocity.x != steering.velocity.x || velocity.z != steering.velocity.z {
                    velocity.x = steering.velocity.x;
                    velocity.z = steering.velocity.z;
                }
                if model.mobility != Mobility::Walk && steering.velocity.y != 0.0 {
                    position.y += steering.velocity.y;
                }
                if let Some(yaw) = steering.yaw
                    && rotation.yaw != yaw
                {
                    rotation.yaw = yaw;
                }
                if steering.jump {
                    vertical.0 = JUMP_VELOCITY;
                    grounded.0 = false;
                }
            }
            FollowStatus::Arrived => {
                halt(&mut velocity);
                navigator.owns_velocity = false;
                arrived(navigator);
            }
            FollowStatus::Stuck => {
                halt(&mut velocity);
                navigator.owns_velocity = false;
                navigator.failures = navigator.failures.saturating_add(1);
                if navigator.failures > settings.max_failures {
                    unreachable(&settings, navigator);
                } else {
                    navigator.clear_path();
                    navigator.phase = Phase::Idle;
                    navigator.request_path();
                }
            }
        }
    }
    stats.moving = moving;
}

fn halt(velocity: &mut Velocity) {
    if velocity.x != 0.0 || velocity.z != 0.0 {
        velocity.x = 0.0;
        velocity.z = 0.0;
    }
}

fn arrived(navigator: &mut Navigator) {
    let complete = navigator.path.is_complete();
    navigator.clear_path();
    navigator.phase = Phase::Idle;
    if !complete {
        navigator.request_path();
        return;
    }
    navigator.failures = 0;
    match navigator.goal_mut() {
        Some(Goal::MoveTo { .. }) => navigator.finish(NavigationOutcome::Reached),
        Some(Goal::Patrol { points, next }) if points.len() > 1 => {
            *next = (*next + 1) % points.len();
            navigator.request_path();
        }
        Some(Goal::Patrol { .. }) | Some(Goal::Follow { .. }) => navigator.phase = Phase::Holding,
        Some(Goal::Flee { .. }) => navigator.request_path(),
        None => {}
    }
}

pub(crate) fn emit_outcomes(
    mut commands: Commands,
    mut navigators: Query<(Entity, &mut Navigator)>,
) {
    for (entity, mut navigator) in &mut navigators {
        if navigator.outcomes.is_empty() {
            continue;
        }
        for outcome in navigator.outcomes.drain(..) {
            commands.trigger(NavigationEvent { entity, outcome });
        }
    }
}

pub(crate) fn release_velocity(
    event: On<Remove, Navigator>,
    mut entities: Query<(&Navigator, &mut Velocity)>,
) {
    if let Ok((navigator, mut velocity)) = entities.get_mut(event.entity)
        && navigator.owns_velocity
    {
        halt(&mut velocity);
    }
}
