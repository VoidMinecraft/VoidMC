use super::grid::NavWorld;
use super::math::{BlockPos, Vec3};
use super::movement::stand;
use super::profile::{Mobility, MoveModel};

const MAX_SHORTCUT: usize = 32;
const CLEARANCE_MARGIN: f64 = 0.1;
const PROBE_SPACING: f64 = 0.25;

/// A followable path: feet positions from the start (excluded) to the end.
/// `complete` is false when the search stopped at its node budget or could
/// not reach the goal and returned the closest point instead.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path {
    points: Vec<Vec3>,
    complete: bool,
}

impl Path {
    pub fn new(points: Vec<Vec3>, complete: bool) -> Self {
        Self { points, complete }
    }

    pub fn points(&self) -> &[Vec3] {
        &self.points
    }

    pub fn is_complete(&self) -> bool {
        self.complete
    }

    pub fn end(&self) -> Option<Vec3> {
        self.points.last().copied()
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    pub fn clear(&mut self) {
        self.points.clear();
        self.complete = false;
    }

    /// Replaces this path with a copy of `other`, reusing the allocation.
    pub fn copy_from(&mut self, other: &Path) {
        self.points.clear();
        self.points.extend_from_slice(&other.points);
        self.complete = other.complete;
    }

    pub(crate) fn begin(&mut self, complete: bool) -> &mut Vec<Vec3> {
        self.points.clear();
        self.complete = complete;
        &mut self.points
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PathNode {
    pub pos: BlockPos,
    pub floor: i32,
}

impl PathNode {
    pub fn point(self) -> Vec3 {
        Vec3::new(
            self.pos.x as f64 + 0.5,
            self.floor as f64 / 16.0,
            self.pos.z as f64 + 0.5,
        )
    }
}

/// String pulling: from each kept node, gallops then bisects to the farthest
/// node still reachable in a straight line, so a run of `n` nodes costs
/// `O(log n)` segment probes. Walkers only shortcut across flat, plain ground
/// so every height change keeps its own waypoint.
pub(crate) fn smooth(
    model: &MoveModel,
    world: &mut impl NavWorld,
    nodes: &[PathNode],
    out: &mut Vec<Vec3>,
) {
    if nodes.len() <= 2 {
        out.extend(nodes.iter().skip(1).map(|node| node.point()));
        return;
    }
    let mut probe = SegmentProbe::default();
    let mut anchor = 0;
    while anchor < nodes.len() - 1 {
        let last = (nodes.len() - 1).min(anchor + MAX_SHORTCUT);
        let mut reach = anchor + 1;
        let mut miss = last + 1;
        let mut stride = 1;
        while reach < last {
            let candidate = (reach + stride).min(last);
            if probe.clear(model, world, nodes[anchor], nodes[candidate]) {
                reach = candidate;
                stride *= 2;
            } else {
                miss = candidate;
                break;
            }
        }
        while miss - reach > 1 {
            let candidate = reach + (miss - reach) / 2;
            if probe.clear(model, world, nodes[anchor], nodes[candidate]) {
                reach = candidate;
            } else {
                miss = candidate;
            }
        }
        out.push(nodes[reach].point());
        anchor = reach;
    }
}

#[derive(Default)]
struct SegmentProbe {
    memo: [(BlockPos, i32, bool); 16],
    filled: usize,
    next: usize,
}

impl SegmentProbe {
    fn clear(
        &mut self,
        model: &MoveModel,
        world: &mut impl NavWorld,
        from: PathNode,
        to: PathNode,
    ) -> bool {
        if model.mobility == Mobility::Walk && from.floor != to.floor {
            return false;
        }
        let a = from.point();
        let b = to.point();
        let length = a.distance(b);
        let samples = (length / PROBE_SPACING).ceil().max(1.0) as usize;
        let reach = model.half_width.max(0.05) + CLEARANCE_MARGIN;
        for i in 1..samples {
            let p = a.lerp(b, i as f64 / samples as f64);
            for (ox, oz) in [
                (-reach, -reach),
                (reach, -reach),
                (-reach, reach),
                (reach, reach),
            ] {
                let feet = BlockPos::containing(Vec3::new(p.x + ox, p.y, p.z + oz));
                if !self.open(model, world, feet, from.floor) {
                    return false;
                }
                if model.mobility != Mobility::Walk && p.y.fract() > 1.0e-6 {
                    let upper = feet.offset(0, 1, 0);
                    if !self.open(model, world, upper, upper.y * 16) {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn open(
        &mut self,
        model: &MoveModel,
        world: &mut impl NavWorld,
        pos: BlockPos,
        floor: i32,
    ) -> bool {
        let filled = self.filled.min(self.memo.len());
        if let Some(&(_, _, open)) = self.memo[..filled]
            .iter()
            .find(|(p, f, _)| *p == pos && *f == floor)
        {
            return open;
        }
        let open = self.evaluate(model, world, pos, floor);
        self.memo[self.next] = (pos, floor, open);
        self.next = (self.next + 1) % self.memo.len();
        self.filled += 1;
        open
    }

    fn evaluate(
        &self,
        model: &MoveModel,
        world: &mut impl NavWorld,
        pos: BlockPos,
        floor: i32,
    ) -> bool {
        let pos = match model.mobility {
            Mobility::Walk => BlockPos::new(pos.x, floor >> 4, pos.z),
            _ => pos,
        };
        let Some(found) = stand(model, world, pos) else {
            return false;
        };
        if found.cost > 1.0 || (model.mobility == Mobility::Walk && found.floor != floor) {
            return false;
        }
        let mut y = found.floor >> 4;
        while y * 16 < found.floor + model.head {
            if world.cell(BlockPos::new(pos.x, y, pos.z)).edges() != 0 {
                return false;
            }
            y += 1;
        }
        true
    }
}
