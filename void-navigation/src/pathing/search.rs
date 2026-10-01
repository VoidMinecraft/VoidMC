use std::cmp::Ordering;
use std::collections::BinaryHeap;

use super::grid::NavWorld;
use super::hash::FastMap;
use super::math::{BlockPos, Vec3};
use super::movement::{Move, heuristic, neighbors, stand};
use super::path::{Path, PathNode, smooth};
use super::profile::{Mobility, MoveModel, NavigationProfile};

const NONE: u32 = u32::MAX;
const START_DROP: i32 = 3;
const GOAL_SNAP: i32 = 8;
const WINDOW_XZ: i32 = 128;
const WINDOW_Y: i32 = 32;

/// Dense node index around the search start. Entries are never cleared: a
/// slot is valid only when it points inside the current node arena at a node
/// with the same position, which is exactly the node for that position.
#[derive(Default)]
struct Window {
    origin: BlockPos,
    slots: Vec<u32>,
}

impl Window {
    #[inline]
    fn slot(&self, pos: BlockPos) -> Option<usize> {
        let x = pos.x - self.origin.x;
        let y = pos.y - self.origin.y;
        let z = pos.z - self.origin.z;
        if (0..WINDOW_XZ).contains(&x) && (0..WINDOW_Y).contains(&y) && (0..WINDOW_XZ).contains(&z)
        {
            Some(((y * WINDOW_XZ + z) * WINDOW_XZ + x) as usize)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SearchRequest {
    pub start: Vec3,
    pub goal: Vec3,
    pub radius: f64,
}

impl SearchRequest {
    pub fn new(start: impl Into<Vec3>, goal: impl Into<Vec3>) -> Self {
        Self {
            start: start.into(),
            goal: goal.into(),
            radius: 0.0,
        }
    }

    pub fn within(mut self, radius: f64) -> Self {
        self.radius = radius.max(0.0);
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchStatus {
    Idle,
    Pending,
    Complete,
    Partial,
    Unreachable,
}

impl SearchStatus {
    pub fn is_done(self) -> bool {
        !matches!(self, SearchStatus::Pending | SearchStatus::Idle)
    }

    pub fn found_path(self) -> bool {
        matches!(self, SearchStatus::Complete | SearchStatus::Partial)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchStats {
    pub expanded: u32,
    pub visited: u32,
}

#[derive(Clone, Copy)]
struct Node {
    pos: BlockPos,
    floor: i32,
    g: f32,
    h: f32,
    parent: u32,
    closed: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Open {
    key: u64,
    node: u32,
}

impl Ord for Open {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .key
            .cmp(&self.key)
            .then_with(|| other.node.cmp(&self.node))
    }
}

impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[inline]
fn open_key(f: f32, h: f32) -> u64 {
    (f.max(0.0).to_bits() as u64) << 32 | h.max(0.0).to_bits() as u64
}

#[derive(Clone, Copy)]
struct Active {
    model: MoveModel,
    limit: u32,
    goal: Vec3,
    goal_block: BlockPos,
    radius_sq: f64,
    vertical: f64,
    best: u32,
    found: Option<u32>,
}

/// A resumable A* over the voxel movement graph. All buffers are kept between
/// searches, so a warmed-up pathfinder allocates nothing. Drive it with
/// [`Pathfinder::start`] then [`Pathfinder::step`] under a node budget, or call
/// [`Pathfinder::find`] to search to completion.
#[derive(Default)]
pub struct Pathfinder {
    nodes: Vec<Node>,
    window: Window,
    index: FastMap<BlockPos, u32>,
    open: BinaryHeap<Open>,
    moves: Vec<Move>,
    chain: Vec<PathNode>,
    active: Option<Active>,
    status: Option<SearchStatus>,
    stats: SearchStats,
}

impl Pathfinder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn status(&self) -> SearchStatus {
        self.status.unwrap_or(SearchStatus::Idle)
    }

    pub fn stats(&self) -> SearchStats {
        self.stats
    }

    pub fn reset(&mut self) {
        self.nodes.clear();
        self.index.clear();
        self.open.clear();
        self.active = None;
        self.status = None;
        self.stats = SearchStats::default();
    }

    #[inline]
    fn lookup(&self, pos: BlockPos) -> Option<u32> {
        match self.window.slot(pos) {
            Some(slot) => {
                let node = self.window.slots[slot];
                self.nodes
                    .get(node as usize)
                    .is_some_and(|n| n.pos == pos)
                    .then_some(node)
            }
            None => self.index.get(&pos).copied(),
        }
    }

    #[inline]
    fn remember(&mut self, pos: BlockPos, node: u32) {
        match self.window.slot(pos) {
            Some(slot) => self.window.slots[slot] = node,
            None => {
                self.index.insert(pos, node);
            }
        }
    }

    /// Searches to completion (bounded by the profile's node limit) and writes
    /// the result into `path`.
    pub fn find(
        &mut self,
        world: &mut impl NavWorld,
        profile: &NavigationProfile,
        request: SearchRequest,
        path: &mut Path,
    ) -> SearchStatus {
        self.start(world, &profile.compile(), request);
        let mut budget = u32::MAX;
        let status = self.step(world, &mut budget);
        if status.found_path() {
            self.write_path(world, path);
        } else {
            path.clear();
        }
        status
    }

    pub fn start(&mut self, world: &mut impl NavWorld, model: &MoveModel, request: SearchRequest) {
        self.reset();
        let (start, floor) = resolve_start(model, world, request.start);
        let goal = resolve_goal(model, world, request.goal);
        let goal_block = BlockPos::containing(goal);
        let radius = request.radius.max(0.75);
        let vertical = match model.mobility {
            Mobility::Walk => request.radius.max(1.25),
            Mobility::Fly | Mobility::Swim => radius,
        };
        let h = heuristic(model, start, goal_block);
        let limit = model.node_limit(h);
        self.nodes.push(Node {
            pos: start,
            floor,
            g: 0.0,
            h,
            parent: NONE,
            closed: false,
        });
        if self.window.slots.is_empty() {
            self.window.slots = vec![NONE; (WINDOW_XZ * WINDOW_XZ * WINDOW_Y) as usize];
        }
        self.window.origin = start.offset(-WINDOW_XZ / 2, -WINDOW_Y / 2, -WINDOW_XZ / 2);
        self.remember(start, 0);
        self.open.push(Open {
            key: open_key(h * model.weight, h),
            node: 0,
        });
        self.active = Some(Active {
            model: *model,
            limit,
            goal,
            goal_block,
            radius_sq: radius * radius,
            vertical,
            best: 0,
            found: None,
        });
        self.status = Some(SearchStatus::Pending);
        self.stats.visited = 1;
    }

    /// Expands up to `budget` nodes, decrementing it, and reports where the
    /// search stands. A `Pending` search resumes on the next call.
    pub fn step(&mut self, world: &mut impl NavWorld, budget: &mut u32) -> SearchStatus {
        let Some(mut active) = self.active else {
            return self.status();
        };
        if self.status().is_done() {
            return self.status();
        }
        let status = self.expand(world, budget, &mut active);
        self.active = Some(active);
        if status.is_done() {
            self.status = Some(status);
        }
        status
    }

    fn expand(
        &mut self,
        world: &mut impl NavWorld,
        budget: &mut u32,
        active: &mut Active,
    ) -> SearchStatus {
        let model = active.model;
        while *budget > 0 {
            let Some(Open { node: current, .. }) = self.open.pop() else {
                return finish(active, None);
            };
            let node = self.nodes[current as usize];
            if node.closed {
                continue;
            }
            self.nodes[current as usize].closed = true;
            *budget -= 1;
            self.stats.expanded += 1;

            if reaches(active, &node) {
                return finish(active, Some(current));
            }
            let best = self.nodes[active.best as usize];
            if node.h < best.h || (node.h == best.h && node.g < best.g) {
                active.best = current;
            }
            if self.stats.expanded >= active.limit {
                return finish(active, None);
            }

            self.moves.clear();
            neighbors(&model, world, node.pos, node.floor, &mut self.moves);
            for index in 0..self.moves.len() {
                let step = self.moves[index];
                let g = node.g + step.cost;
                let slot = match self.lookup(step.pos) {
                    Some(slot) => {
                        let existing = &mut self.nodes[slot as usize];
                        if existing.closed || g >= existing.g {
                            continue;
                        }
                        existing.g = g;
                        existing.floor = step.floor;
                        existing.parent = current;
                        slot
                    }
                    None => {
                        let slot = self.nodes.len() as u32;
                        let h = heuristic(&model, step.pos, active.goal_block);
                        self.nodes.push(Node {
                            pos: step.pos,
                            floor: step.floor,
                            g,
                            h,
                            parent: current,
                            closed: false,
                        });
                        self.remember(step.pos, slot);
                        self.stats.visited += 1;
                        slot
                    }
                };
                let h = self.nodes[slot as usize].h;
                self.open.push(Open {
                    key: open_key(g + h * model.weight, h),
                    node: slot,
                });
            }
        }
        SearchStatus::Pending
    }

    /// Writes the found (or closest, for `Partial`) path, smoothed.
    pub fn write_path(&mut self, world: &mut impl NavWorld, path: &mut Path) {
        let Some(active) = self.active.as_ref() else {
            path.clear();
            return;
        };
        let status = self.status();
        let end = match status {
            SearchStatus::Complete => active.found.unwrap_or(active.best),
            SearchStatus::Partial => active.best,
            _ => {
                path.clear();
                return;
            }
        };
        self.chain.clear();
        let mut cursor = end;
        while cursor != NONE {
            let node = self.nodes[cursor as usize];
            self.chain.push(PathNode {
                pos: node.pos,
                floor: node.floor,
            });
            cursor = node.parent;
        }
        self.chain.reverse();
        let points = path.begin(status == SearchStatus::Complete);
        smooth(&active.model, world, &self.chain, points);
        if points.is_empty() && status == SearchStatus::Complete {
            points.push(self.chain[0].point());
        }
    }
}

fn finish(active: &mut Active, found: Option<u32>) -> SearchStatus {
    active.found = found;
    if found.is_some() {
        SearchStatus::Complete
    } else if active.model.allow_partial && active.best != 0 {
        SearchStatus::Partial
    } else {
        SearchStatus::Unreachable
    }
}

fn reaches(active: &Active, node: &Node) -> bool {
    let point = Vec3::new(
        node.pos.x as f64 + 0.5,
        node.floor as f64 / 16.0,
        node.pos.z as f64 + 0.5,
    );
    let dx = point.x - active.goal.x;
    let dz = point.z - active.goal.z;
    let dy = point.y - active.goal.y;
    match active.model.mobility {
        Mobility::Walk => dx * dx + dz * dz <= active.radius_sq && dy.abs() <= active.vertical,
        Mobility::Fly | Mobility::Swim => dx * dx + dy * dy + dz * dz <= active.radius_sq,
    }
}

/// Walkers can only end on a floor: a goal floating above or buried below
/// the ground is moved to the nearest standable height of its column.
fn resolve_goal(model: &MoveModel, world: &mut impl NavWorld, goal: Vec3) -> Vec3 {
    if model.mobility != Mobility::Walk {
        return goal;
    }
    let block = BlockPos::containing(Vec3::new(goal.x, goal.y + 1.0e-3, goal.z));
    let offsets = std::iter::once(0).chain((1..=GOAL_SNAP).flat_map(|d| [-d, d]));
    for dy in offsets {
        if let Some(found) = stand(model, world, block.offset(0, dy, 0)) {
            return Vec3::new(goal.x, found.floor as f64 / 16.0, goal.z);
        }
    }
    goal
}

fn resolve_start(model: &MoveModel, world: &mut impl NavWorld, start: Vec3) -> (BlockPos, i32) {
    let feet = BlockPos::containing(Vec3::new(start.x, start.y + 1.0e-3, start.z));
    for drop in 0..=START_DROP {
        let pos = feet.offset(0, -drop, 0);
        if let Some(found) = stand(model, world, pos) {
            return (pos, found.floor);
        }
    }
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        let pos = feet.offset(dx, 0, dz);
        if let Some(found) = stand(model, world, pos) {
            return (pos, found.floor);
        }
    }
    (feet, (start.y * 16.0).floor() as i32)
}
