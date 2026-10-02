use std::collections::VecDeque;
use std::time::Instant;

use bevy_ecs::lifecycle::Remove;
use bevy_ecs::prelude::*;
use voidmc::components::{
    EntityDimension, Grounded, PlayerDimension, Position, Rotation, Velocity, VerticalVelocity,
};
use voidmc::world::{ChunkPos, DimensionId};

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
const MAX_REQUESTS_PER_TICK: u32 = 64;
const LONG_LANES: usize = 2;
const CACHE_MARGIN: f64 = 2.0;

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
    stamp: u64,
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
    stamp: u64,
    min: ChunkPos,
    max: ChunkPos,
}

#[derive(Default)]
struct Lane {
    pathfinder: Pathfinder,
    active: Option<ActiveSearch>,
}

/// The shared pathfinders and the FIFO of navigators waiting for a path. A
/// search still running at the end of a tick moves to a long lane, which
/// shares a quarter of the budget, so it never holds up everyone else.
#[derive(Resource, Default)]
pub struct PathPlanner {
    main: Lane,
    long: [Lane; LONG_LANES],
    queue: VecDeque<Entity>,
    cache: FastMap<PathKey, CachedPath>,
}

impl PathPlanner {
    pub fn queued(&self) -> usize {
        let running = std::iter::once(&self.main)
            .chain(&self.long)
            .filter(|lane| lane.active.is_some())
            .count();
        self.queue.len() + running
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

enum Request {
    Skip,
    Served,
    Search {
        me: Vec3,
        dimension: DimensionId,
        request: SearchRequest,
        key: Option<PathKey>,
    },
}

fn prepare(
    entity: Entity,
    navigator: &mut Navigator,
    cache: &FastMap<PathKey, CachedPath>,
    world: &NavigationWorld,
    bodies: &Bodies,
    stats: &mut NavigationStats,
    commit: bool,
) -> Request {
    if !navigator.wants_path || navigator.is_paused() {
        navigator.queued = false;
        return Request::Skip;
    }
    let Some((me, dimension)) = locate(bodies, entity) else {
        navigator.queued = false;
        return Request::Skip;
    };
    let Some(plan) = request_for(navigator, me, bodies) else {
        navigator.queued = false;
        navigator.finish(NavigationOutcome::Interrupted);
        return Request::Skip;
    };
    let key = plan.fixed.then(|| PathKey {
        dimension,
        start: BlockPos::containing(me),
        goal: BlockPos::containing(plan.request.goal),
        radius: plan.request.radius.to_bits(),
        model: navigator.fingerprint,
    });
    let hit = key
        .and_then(|key| cache.get(&key))
        .filter(|hit| !world.changed_since(dimension, hit.min, hit.max, hit.stamp));
    if let Some(hit) = hit {
        navigator.queued = false;
        navigator.wants_path = false;
        navigator.planned_for = Some(plan.planned_for);
        navigator.path.copy_from(&hit.path);
        navigator.origin = me;
        navigator.path_changed();
        navigator.phase = Phase::Following;
        stats.cache_hits += 1;
        return Request::Served;
    }
    if commit {
        navigator.wants_path = false;
        navigator.planned_for = Some(plan.planned_for);
    }
    Request::Search {
        me,
        dimension,
        request: plan.request,
        key,
    }
}

fn chunk_box(origin: Vec3, path: &Path) -> (ChunkPos, ChunkPos) {
    let (mut lo, mut hi) = (origin, origin);
    for point in path.points() {
        lo = Vec3::new(lo.x.min(point.x), 0.0, lo.z.min(point.z));
        hi = Vec3::new(hi.x.max(point.x), 0.0, hi.z.max(point.z));
    }
    let chunk = |v: f64| (v.floor() as i32) >> 4;
    (
        ChunkPos::new(chunk(lo.x - CACHE_MARGIN), chunk(lo.z - CACHE_MARGIN)),
        ChunkPos::new(chunk(hi.x + CACHE_MARGIN), chunk(hi.z + CACHE_MARGIN)),
    )
}

fn is_current(navigators: &Query<&mut Navigator>, active: &ActiveSearch) -> bool {
    navigators
        .get(active.entity)
        .is_ok_and(|navigator| navigator.search_ticket == active.ticket)
}

fn release(navigators: &mut Query<&mut Navigator>, entity: Entity) {
    if let Ok(mut navigator) = navigators.get_mut(entity) {
        navigator.queued = false;
    }
}

fn advance(
    pathfinder: &mut Pathfinder,
    world: &mut NavigationWorld,
    chunks: &ChunkCells,
    dimension: DimensionId,
    budget: &mut u32,
    stats: &mut NavigationStats,
) -> SearchStatus {
    let before = *budget;
    let status = pathfinder.step(&mut world.view(dimension, chunks), budget);
    stats.expanded_last_tick += before - *budget;
    stats.expanded += (before - *budget) as u64;
    status
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
    let world = world.as_mut();
    let stats = stats.as_mut();
    stats.expanded_last_tick = 0;

    let mut scanned = 0;
    let PathPlanner { queue, cache, .. } = planner;
    queue.retain(|&entity| {
        if scanned >= MAX_REQUESTS_PER_TICK {
            return true;
        }
        scanned += 1;
        let Ok(mut navigator) = navigators.get_mut(entity) else {
            return false;
        };
        matches!(
            prepare(entity, &mut navigator, cache, world, &bodies, stats, false),
            Request::Search { .. }
        )
    });

    let total = settings.expansions_per_tick;
    let long_running = planner
        .long
        .iter()
        .filter(|lane| lane.active.is_some())
        .count();
    let long_share = if long_running > 0 {
        (total / 4).max(1).min(total)
    } else {
        0
    };
    let mut budget = total - long_share;
    let mut requests = 0;
    while budget > 0 {
        let active = match planner.main.active {
            Some(active) => active,
            None => {
                if requests >= MAX_REQUESTS_PER_TICK {
                    break;
                }
                let Some(entity) = planner.queue.pop_front() else {
                    break;
                };
                requests += 1;
                let Ok(mut navigator) = navigators.get_mut(entity) else {
                    continue;
                };
                let Request::Search {
                    me,
                    dimension,
                    request,
                    key,
                } = prepare(
                    entity,
                    &mut navigator,
                    &planner.cache,
                    world,
                    &bodies,
                    stats,
                    true,
                )
                else {
                    continue;
                };
                let model = navigator.model;
                planner
                    .main
                    .pathfinder
                    .start(&mut world.view(dimension, &chunks), &model, request);
                stats.searches += 1;
                let active = ActiveSearch {
                    entity,
                    ticket: navigator.search_ticket,
                    dimension,
                    origin: me,
                    key,
                    stamp: world.stamp(),
                };
                planner.main.active = Some(active);
                active
            }
        };
        if !is_current(&navigators, &active) {
            planner.main.active = None;
            release(&mut navigators, active.entity);
            continue;
        }
        let status = advance(
            &mut planner.main.pathfinder,
            world,
            &chunks,
            active.dimension,
            &mut budget,
            stats,
        );
        if status == SearchStatus::Pending {
            break;
        }
        planner.main.active = None;
        conclude(
            status,
            active,
            &mut planner.main.pathfinder,
            &mut planner.cache,
            world,
            &chunks,
            &bodies,
            &mut navigators,
            &settings,
            stats,
        );
    }
    if planner.main.active.is_some()
        && let Some(lane) = planner.long.iter_mut().find(|lane| lane.active.is_none())
    {
        std::mem::swap(&mut planner.main, lane);
    }

    let mut long_budget = long_share + budget;
    let lanes = planner
        .long
        .iter()
        .filter(|lane| lane.active.is_some())
        .count() as u32;
    for lane in &mut planner.long {
        let Some(active) = lane.active else {
            continue;
        };
        if !is_current(&navigators, &active) {
            lane.active = None;
            release(&mut navigators, active.entity);
            continue;
        }
        let mut slice = (long_budget / lanes.max(1)).max(1).min(long_budget);
        if slice == 0 {
            break;
        }
        long_budget -= slice;
        let status = advance(
            &mut lane.pathfinder,
            world,
            &chunks,
            active.dimension,
            &mut slice,
            stats,
        );
        long_budget += slice;
        if status != SearchStatus::Pending {
            lane.active = None;
            conclude(
                status,
                active,
                &mut lane.pathfinder,
                &mut planner.cache,
                world,
                &chunks,
                &bodies,
                &mut navigators,
                &settings,
                stats,
            );
        }
    }
    stats.queued = planner.queued();
    stats.planning_micros_last_tick = started.elapsed().as_micros() as u64;
}

#[allow(clippy::too_many_arguments)]
fn conclude(
    status: SearchStatus,
    active: ActiveSearch,
    pathfinder: &mut Pathfinder,
    cache: &mut FastMap<PathKey, CachedPath>,
    world: &mut NavigationWorld,
    chunks: &ChunkCells,
    bodies: &Bodies,
    navigators: &mut Query<&mut Navigator>,
    settings: &NavigationSettings,
    stats: &mut NavigationStats,
) {
    let Ok(mut navigator) = navigators.get_mut(active.entity) else {
        return;
    };
    navigator.queued = false;
    if navigator.search_ticket != active.ticket {
        return;
    }
    let navigator = navigator.as_mut();
    if status.found_path() {
        pathfinder.write_path(
            &mut world.view(active.dimension, chunks),
            &mut navigator.path,
        );
        navigator.origin = active.origin;
        navigator.path_changed();
    }
    let progress = match (status, navigator.path.end(), locate(bodies, active.entity)) {
        (SearchStatus::Partial, Some(end), Some((me, _))) => end.horizontal_distance(me) > 1.0,
        (status, _, _) => status == SearchStatus::Complete,
    };
    if !progress {
        stats.unreachable += 1;
        unreachable(settings, navigator);
        return;
    }
    navigator.phase = Phase::Following;
    if status != SearchStatus::Complete {
        stats.partial += 1;
        return;
    }
    stats.complete += 1;
    let Some(key) = active.key else {
        return;
    };
    if cache.len() >= PATH_CACHE_CAPACITY {
        cache.clear();
    }
    let (min, max) = chunk_box(active.origin, &navigator.path);
    match cache.get_mut(&key) {
        Some(cached) => {
            cached.path.copy_from(&navigator.path);
            cached.stamp = active.stamp;
            cached.min = min;
            cached.max = max;
        }
        None => {
            cache.insert(
                key,
                CachedPath {
                    path: navigator.path.clone(),
                    stamp: active.stamp,
                    min,
                    max,
                },
            );
        }
    }
}

fn unreachable(settings: &NavigationSettings, navigator: &mut Navigator) {
    let still_following = navigator.phase == Phase::Following && !navigator.path.is_empty();
    if still_following && matches!(navigator.goal(), Some(Goal::Follow { .. })) {
        navigator.repath_cooldown = RETRY_TICKS;
        return;
    }
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
        &Grounded,
    )>,
) {
    let mut moving = 0;
    for (mut navigator, mut position, mut velocity, mut rotation, mut vertical, grounded) in
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
                if navigator.failures >= settings.max_failures {
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
