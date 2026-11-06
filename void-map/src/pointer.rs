//! What the player aims at, from where, and how the mouse moves it.
//!
//! The overview stands south of the model and well above it. Aiming straight down
//! is then off the model, so no one needs the angle where a look spins on itself.

use crate::motion::{Follower, Motion};
use crate::scene::SEA_Y;

/// How steeply the overview looks down. Everything about it follows from this.
pub const REST_PITCH: f32 = 55.0;
/// North.
pub const REST_YAW: f32 = 180.0;
/// South of the model's centre, past its rim: aiming straight down is off the land.
pub const OVERVIEW_Z: f64 = 65.0;
/// Where the land ends, whatever the rim does.
const LAND_RADIUS: f64 = 54.0;
pub const SPECTATOR_EYE_HEIGHT: f64 = 1.62;

/// The detail view is close to straight down and low, because a far look runs out of
/// what a render distance of 8 shows.
/// An entity's rotation travels as 1/256 of a turn: this is what the camera really has.
pub const DETAIL_PITCH: f32 = 56.0 * 360.0 / 256.0;
/// Heights of the view above the sea, in blocks of the detail model. Past
/// [`COARSE_ABOVE`] the view moves to the coarser model and stays under the fog that
/// ends a render distance of 8: a height there is one the detail model would have.
pub const CLOSEST_ZOOM: f64 = 40.0;
pub const FARTHEST_ZOOM: f64 = 280.0;
pub const DEFAULT_ZOOM: f64 = 95.0;
pub const COARSE_ABOVE: f64 = 105.0;
/// From the middle model to the overview, and back: the same reasoning, one step up.
pub const COARSEST_ABOVE: f64 = 175.0;
pub const MIDDLE_BELOW: f64 = 160.0;
/// Back to the detail model. The gap stops a held height from flipping between the two.
pub const DETAIL_BELOW: f64 = 96.0;
const _: () = assert!(DETAIL_BELOW < COARSE_ABOVE, "a steady zoom needs a gap");
const _: () = assert!(MIDDLE_BELOW < COARSEST_ABOVE, "a steady zoom needs a gap");
const _: () = assert!(COARSE_ABOVE < MIDDLE_BELOW, "a model has to have its turn");
/// What a tick with the key held multiplies the height by. It feels the same at any height.
const ZOOM_PER_TICK: f64 = 1.03;

/// Where the look comes back to once the mouse rests. It goes past these while moving.
const LOOK_YAW_LIMIT: f32 = 95.0;
const LOOK_PITCH_LIMITS: [f32; 2] = [15.0, 90.0];
/// How long the mouse has to rest before the view drifts back, in ticks.
const SETTLE_TICKS: u32 = 10;
/// How much of what is left the look drifts back by in a tick, and in degrees at most.
const LOOK_DRIFT_SHARE: f32 = 0.12;
const LOOK_DRIFT_MOST: f32 = 1.2;
/// Under this, the look has arrived.
const LOOK_DRIFT_LEAST: f32 = 0.1;

/// Past this pitch a rider is turned back, well before the ±90° stop that would
/// swallow mouse movement.
const RECENTRE_PITCH: f32 = 45.0;
/// Where a rider is turned back to. The client answers at once with exactly this.
pub const RECENTRED_PITCH: f32 = 0.0;
/// How fast the mouse pans, the same at any speed.
const BLOCKS_PER_DEGREE: f64 = 5.0;
/// As far as a report can move the view: chunks can follow, and the model stays whole.
pub const MAX_PAN_STEP: f64 = 40.0;
/// How far past the rim a pan can stretch. The further out, the harder it pulls.
const PAN_MARGIN: i32 = 28;
/// How much of the way to its aim a height covers in a tick.
const VIEW_SHARE: f64 = 0.45;
/// How much of its speed a view keeps from tick to tick, with inertia on, and where it
/// lets go.
const COAST_KEEP: f64 = 0.85;
const COAST_OVER: f64 = 0.05;
/// A client that finds an entity further than this from where it is told to go jumps
/// there instead of gliding, then glides back to what it was still doing.
pub const CLIENT_SNAP_DISTANCE: f64 = 64.0;
/// The most the view moves in a tick. A camera glides a few ticks behind it, and has to
/// stay clear of [`CLIENT_SNAP_DISTANCE`] however fast the mouse goes.
pub const MAX_VIEW_STEP: f64 = 12.0;
/// A client is this many ticks behind at worst: the glide, and one for the network.
const CLIENT_LAG_TICKS: f64 = 4.0;
const _: () = assert!(
    MAX_VIEW_STEP * CLIENT_LAG_TICKS < CLIENT_SNAP_DISTANCE,
    "a fast pan would make the client jump"
);
/// How far the aim runs ahead of the view, so that turning back answers at once.
const MAX_AIM_LEAD: f64 = MAX_VIEW_STEP / VIEW_SHARE;
const VIEW_ARRIVED: f64 = 0.01;

/// How far off the middle of the screen a marker gets picked up, and let go of.
const AIM_ENTER: f64 = 4.0;
const AIM_LEAVE: f64 = 6.0;
/// How much closer to the middle another marker must be to take over.
const AIM_SWITCH: f64 = 1.5;

/// Height of the overview's eye over the sea. It puts the near and far rims at equal
/// angles from the middle of the screen, which centres the land in the frame.
fn overview_eye_height() -> f64 {
    let (near, far) = (OVERVIEW_Z - LAND_RADIUS, OVERVIEW_Z + LAND_RADIUS);
    let double = 2.0 * f64::from(REST_PITCH).to_radians();
    let spread = (near + far) * double.cos() / double.sin();
    (-spread + (spread * spread + 4.0 * near * far).sqrt()) / 2.0
}

/// Where a spectator stands in the overview.
pub fn overview_y() -> f64 {
    f64::from(SEA_Y) + overview_eye_height() - SPECTATOR_EYE_HEIGHT
}

/// How far south of the point it aims at the detail view stands, from a height.
pub fn detail_back(height: f64) -> f64 {
    height / f64::from(DETAIL_PITCH).to_radians().tan()
}

pub fn shortest_turn(from: f32, to: f32) -> f32 {
    (to - from + 180.0).rem_euclid(360.0) - 180.0
}

/// Unit vector of a look, in Minecraft degrees.
pub fn look_direction(yaw: f32, pitch: f32) -> [f64; 3] {
    let (yaw, pitch) = (f64::from(yaw).to_radians(), f64::from(pitch).to_radians());
    [
        -yaw.sin() * pitch.cos(),
        -pitch.sin(),
        yaw.cos() * pitch.cos(),
    ]
}

/// How far off the middle of the screen something gets picked up, in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AimReach {
    pub enter: f64,
    pub leave: f64,
    /// How much closer to the middle another one must be to take over.
    pub switch: f64,
}

const MARKER_REACH: AimReach = AimReach {
    enter: AIM_ENTER,
    leave: AIM_LEAVE,
    switch: AIM_SWITCH,
};

/// Degrees between a look and a point.
fn angle_to(eye: [f64; 3], direction: [f64; 3], point: [f64; 3]) -> f64 {
    let to_point: [f64; 3] = std::array::from_fn(|axis| point[axis] - eye[axis]);
    let length = to_point.iter().map(|v| v * v).sum::<f64>().sqrt();
    let along: f64 = (0..3).map(|axis| to_point[axis] * direction[axis]).sum();
    (along / length.max(f64::EPSILON))
        .clamp(-1.0, 1.0)
        .acos()
        .to_degrees()
}

/// Whether a look is close enough to a marker for it to matter, hovered or not: what is
/// around a marker goes to that marker.
pub fn near_marker(eye: [f64; 3], direction: [f64; 3], point: [f64; 3]) -> bool {
    angle_to(eye, direction, point) <= AIM_LEAVE
}

/// The marker aimed at, by its angle from the middle of the screen. The one already
/// aimed at holds on for a while, so that the choice never flickers between two.
pub fn aimed(
    eye: [f64; 3],
    direction: [f64; 3],
    points: impl IntoIterator<Item = [f64; 3]>,
    current: Option<usize>,
) -> Option<usize> {
    aimed_within(MARKER_REACH, eye, direction, points, current)
}

/// The same for anything, with its own reach.
pub fn aimed_within(
    reach: AimReach,
    eye: [f64; 3],
    direction: [f64; 3],
    points: impl IntoIterator<Item = [f64; 3]>,
    current: Option<usize>,
) -> Option<usize> {
    let angles: Vec<f64> = points
        .into_iter()
        .map(|point| angle_to(eye, direction, point))
        .collect();
    let nearest = (0..angles.len()).min_by(|a, b| angles[*a].total_cmp(&angles[*b]));
    let held = current.filter(|index| angles.get(*index).is_some_and(|a| *a <= reach.leave));
    let picked = nearest.filter(|index| angles[*index] <= reach.enter);
    match (held, picked) {
        (Some(held), Some(picked)) if angles[picked] + reach.switch < angles[held] => Some(picked),
        (Some(held), _) => Some(held),
        (None, picked) => picked,
    }
}

/// Where a look comes back to, as the nearest angles within the limits.
pub fn resting_look(yaw: f32, pitch: f32) -> [f32; 2] {
    let off = shortest_turn(REST_YAW, yaw);
    let held = off.clamp(-LOOK_YAW_LIMIT, LOOK_YAW_LIMIT);
    [
        yaw + (held - off),
        pitch.clamp(LOOK_PITCH_LIMITS[0], LOOK_PITCH_LIMITS[1]),
    ]
}

/// Follows a free look, which the mouse alone moves. The client turns it many times
/// between two server ticks, so correcting a look in motion can only fight the mouse.
/// Past its limits a look drifts back in small steps, and only once the mouse rests.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FreeLook {
    last: Option<[f32; 2]>,
    /// Ticks since the mouse last moved.
    still: u32,
    /// Where the last step should put the look: reaching it is no mouse movement.
    awaited: Option<[f32; 2]>,
}

fn same_look(a: [f32; 2], b: [f32; 2]) -> bool {
    shortest_turn(a[0], b[0]).abs() < 0.01 && (a[1] - b[1]).abs() < 0.01
}

impl FreeLook {
    pub fn report(&mut self, yaw: f32, pitch: f32) {
        let look = [yaw, pitch];
        let last = self.last.replace(look);
        if self.awaited.is_some_and(|awaited| same_look(awaited, look)) {
            self.awaited = None;
        } else if last.is_none_or(|last| !same_look(last, look)) {
            self.still = 0;
            self.awaited = None;
        }
    }

    /// Whether the look stands past its limits.
    pub fn is_past(&self) -> bool {
        self.last
            .is_some_and(|[yaw, pitch]| !same_look(resting_look(yaw, pitch), [yaw, pitch]))
    }

    /// The step to add to the look on this tick, as `[yaw, pitch]`.
    pub fn tick(&mut self) -> Option<[f32; 2]> {
        self.still = self.still.saturating_add(1);
        let [yaw, pitch] = self.last?;
        if self.still < SETTLE_TICKS || self.awaited.is_some() {
            return None;
        }
        let [to_yaw, to_pitch] = resting_look(yaw, pitch);
        let left = [shortest_turn(yaw, to_yaw), to_pitch - pitch];
        if left[0].hypot(left[1]) < 0.01 {
            return None;
        }
        let step = left.map(|left| {
            let wanted = (left * LOOK_DRIFT_SHARE).clamp(-LOOK_DRIFT_MOST, LOOK_DRIFT_MOST);
            if left.abs() < LOOK_DRIFT_LEAST {
                left
            } else if wanted.abs() < LOOK_DRIFT_LEAST {
                LOOK_DRIFT_LEAST.copysign(left)
            } else {
                wanted
            }
        });
        self.awaited = Some([yaw + step[0], pitch + step[1]]);
        Some(step)
    }
}

/// A height of the view that follows a key held down, and the height shown on its way.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Zoom {
    wanted: f64,
    shown: f64,
    /// What `shown` was a tick ago.
    last: f64,
}

impl Default for Zoom {
    fn default() -> Self {
        Self {
            wanted: DEFAULT_ZOOM,
            shown: DEFAULT_ZOOM,
            last: DEFAULT_ZOOM,
        }
    }
}

impl Zoom {
    /// Takes one tick with these keys held.
    pub fn tick(&mut self, out: bool, closer: bool) {
        if out != closer {
            let change = if out {
                ZOOM_PER_TICK
            } else {
                1.0 / ZOOM_PER_TICK
            };
            self.wanted = (self.wanted * change).clamp(CLOSEST_ZOOM, FARTHEST_ZOOM);
        }
        self.last = self.shown;
        self.shown = glide_height(self.shown, self.wanted);
    }

    pub fn height(self) -> f64 {
        self.shown
    }

    /// The height this many ticks ago, going by how fast it changes now.
    pub fn height_behind(self, ticks: f64) -> f64 {
        self.shown * (self.last / self.shown).powf(ticks)
    }
}

/// Most of the way there, in one number: a height has no speed to keep to.
fn glide_height(from: f64, to: f64) -> f64 {
    if (to - from).abs() < VIEW_ARRIVED {
        to
    } else {
        from + (to - from) * VIEW_SHARE
    }
}

/// What one rotation report amounts to.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Turn {
    /// Degrees turned since the last report, as `[yaw, pitch]`.
    pub delta: [f32; 2],
    /// Pitch to add to the player, relatively, to bring them back.
    pub recentre: bool,
}

/// Follows the rotation a rider reports, whose look is never shown.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Mouse {
    last: Option<[f32; 2]>,
    /// A recentring was sent. The client answers it with exactly [`RECENTRED_PITCH`],
    /// and every report until then is real movement, however large.
    awaited: bool,
}

impl Mouse {
    pub fn report(&mut self, yaw: f32, pitch: f32) -> Turn {
        let last = self.last.replace([yaw, pitch]);
        let answer = self.awaited && pitch == RECENTRED_PITCH;
        self.awaited &= !answer;
        let delta = last.map_or([0.0; 2], |[last_yaw, last_pitch]| {
            let moved = if answer { 0.0 } else { pitch - last_pitch };
            [shortest_turn(last_yaw, yaw), moved]
        });
        let recentre = !self.awaited && pitch.abs() > RECENTRE_PITCH;
        self.awaited |= recentre;
        Turn { delta, recentre }
    }
}

const AROUND: [[f64; 2]; 8] = [
    [1.0, 0.0],
    [-1.0, 0.0],
    [0.0, 1.0],
    [0.0, -1.0],
    [0.707, 0.707],
    [0.707, -0.707],
    [-0.707, 0.707],
    [-0.707, -0.707],
];

/// How far off the model a point is, at most [`PAN_MARGIN`], and the way back to it.
fn off_model(at: [f64; 2], on_model: &impl Fn([f64; 2]) -> bool) -> (f64, [f64; 2]) {
    if on_model(at) {
        return (0.0, [0.0; 2]);
    }
    for distance in 1..PAN_MARGIN {
        let blocks = f64::from(distance);
        let way = AROUND
            .iter()
            .find(|[x, z]| on_model([at[0] + x * blocks, at[1] + z * blocks]));
        if let Some(way) = way {
            return (blocks, *way);
        }
    }
    (f64::from(PAN_MARGIN), [0.0; 2])
}

/// How much of a step outwards goes through, this far past the rim. It starts and ends
/// flat, so that neither end shows as a jolt.
fn rim_give(past: f64) -> f64 {
    let share = (past / f64::from(PAN_MARGIN)).clamp(0.0, 1.0);
    1.0 - share * share * (3.0 - 2.0 * share)
}

/// Where a step from a point ends up. Past the rim of what `on_model` accepts, moving out
/// gets slower and slower, up to a margin nothing goes past. Moving back stays free.
fn through_rim(from: [f64; 2], step: [f64; 2], on_model: &impl Fn([f64; 2]) -> bool) -> [f64; 2] {
    let mut at = from;
    for (axis, step) in step.into_iter().enumerate() {
        let moves = step.abs().ceil().max(1.0) as u32;
        let part = step / f64::from(moves);
        for _ in 0..moves {
            let mut next = at;
            next[axis] += part;
            let (past, way) = off_model(next, on_model);
            let outwards = past > 0.0 && (way == [0.0; 2] || part * way[axis] < 0.0);
            let give = if outwards { rim_give(past) } else { 1.0 };
            at[axis] += part * give;
        }
    }
    at
}

/// Where the mouse has moved a panning view, in blocks from its resting place.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pan {
    /// How the view follows the aim.
    motion: Motion,
    /// Goes on for a moment once the mouse stops.
    inertia: bool,
    follower: Follower,
    aim: [f64; 2],
    /// Where the view is on its way to the aim.
    view: [f64; 2],
    /// How far the view went on the last tick.
    step: [f64; 2],
    /// How far the mouse went since the last tick.
    moved: [f64; 2],
    /// What inertia still has to give, in blocks per tick, once it has started.
    coast: Option<[f64; 2]>,
}

impl Pan {
    pub fn at(offset: [f64; 2]) -> Self {
        Self {
            aim: offset,
            view: offset,
            ..Self::default()
        }
    }

    /// Changes how the view follows, from where it is.
    pub fn drive(&mut self, motion: Motion, inertia: bool) {
        if (self.motion, self.inertia) != (motion, inertia) {
            *self = Self {
                motion,
                inertia,
                ..Self::at(self.view)
            };
        }
    }

    /// Puts the view there, for what moves it some other way than the mouse. A client
    /// glides there, so it has to stay within a step of where the view was.
    pub fn place(&mut self, at: [f64; 2]) {
        self.step = [at[0] - self.view[0], at[1] - self.view[1]];
        self.view = at;
        self.aim = at;
        self.moved = [0.0; 2];
        self.coast = None;
    }

    /// The view looks north: turning right is east, looking down is south.
    pub fn apply(&mut self, delta: [f32; 2]) {
        let step = delta.map(|degrees| f64::from(degrees) * BLOCKS_PER_DEGREE);
        let length = step[0].hypot(step[1]);
        let slowed = (MAX_PAN_STEP / length.max(f64::EPSILON)).min(1.0);
        let aim: [f64; 2] = std::array::from_fn(|axis| self.aim[axis] + step[axis] * slowed);
        let lead = [aim[0] - self.view[0], aim[1] - self.view[1]];
        let kept = (MAX_AIM_LEAD / lead[0].hypot(lead[1]).max(f64::EPSILON)).min(1.0);
        let aim = [self.view[0] + lead[0] * kept, self.view[1] + lead[1] * kept];
        self.moved = std::array::from_fn(|axis| self.moved[axis] + aim[axis] - self.aim[axis]);
        self.aim = aim;
    }

    /// Moves the view on a tick. What the rim holds back is taken off the aim as well: a
    /// view at the rim has nothing stored up, so it never springs back, and turning round
    /// answers at once.
    pub fn tick(&mut self, on_model: impl Fn([f64; 2]) -> bool) {
        let moved = std::mem::take(&mut self.moved);
        let moved = self.coasted(moved);
        let wanted = self
            .follower
            .next(self.motion, self.view, self.step, self.aim, moved);
        let step = [wanted[0] - self.view[0], wanted[1] - self.view[1]];
        let kept = (MAX_VIEW_STEP / step[0].hypot(step[1]).max(f64::EPSILON)).min(1.0);
        let step = step.map(|step| step * kept);
        let at = through_rim(self.view, step, &on_model);
        for axis in 0..2 {
            self.step[axis] = at[axis] - self.view[axis];
            self.aim[axis] -= step[axis] - self.step[axis];
            self.view[axis] = at[axis];
            // Stopped past the aim, the view stays: going back would read as a rollback.
            let past = (self.aim[axis] - at[axis]) * self.step[axis] <= 0.0;
            if past && self.motion.runs_past() && self.follower.stopped() {
                self.aim[axis] = at[axis];
            }
        }
    }

    /// What the mouse moved, with what inertia adds to the aim once it stops.
    fn coasted(&mut self, moved: [f64; 2]) -> [f64; 2] {
        if moved != [0.0; 2] {
            self.coast = None;
            return moved;
        }
        if !self.inertia || (self.coast.is_none() && !self.follower.stopped()) {
            return moved;
        }
        let speed = self
            .coast
            .unwrap_or(self.step)
            .map(|speed| speed * COAST_KEEP);
        let speed = if speed[0].hypot(speed[1]) < COAST_OVER {
            [0.0; 2]
        } else {
            speed
        };
        self.coast = Some(speed);
        self.aim = [self.aim[0] + speed[0], self.aim[1] + speed[1]];
        speed
    }

    /// Where the middle of the screen is headed.
    pub fn offset(self) -> [f64; 2] {
        self.aim
    }

    /// Where the middle of the screen is.
    pub fn view(self) -> [f64; 2] {
        self.view
    }
}

/// A dive into a waypoint lasts this many ticks.
pub const DIVE_TICKS: u32 = 30;
/// It ends this far from the waypoint, along the look, in blocks of a build.
pub const DIVE_STOP: f64 = 16.0;

/// Starts and ends at rest, with no jolt at either end.
pub fn ease(share: f64) -> f64 {
    share.powi(3) * (share * (share * 6.0 - 15.0) + 10.0)
}

#[cfg(test)]
mod tests {
    use voidmc::DefaultWorldGenerator;

    use super::*;

    /// The view is what the client follows. Past a certain speed it jumped back and
    /// forth by tens of blocks, on quick turns back above all.
    #[test]
    fn the_view_never_outruns_what_a_client_can_glide_to() {
        let mut pan = Pan::default();
        for tick in 0..200 {
            let way = if (tick / 4) % 2 == 0 { 40.0 } else { -40.0 };
            let before = pan.view();
            pan.apply([way, way / 2.0]);
            pan.tick(|_| true);
            let after = pan.view();
            let moved = (after[0] - before[0]).hypot(after[1] - before[1]);
            assert!(moved <= MAX_VIEW_STEP + 1e-9, "{moved} on tick {tick}");
        }
    }

    #[test]
    fn turning_back_answers_within_a_few_ticks() {
        let mut pan = Pan::default();
        for _ in 0..20 {
            pan.apply([40.0, 0.0]);
            pan.tick(|_| true);
        }
        let mut last = pan.view()[0];
        let mut ticks = 0;
        loop {
            pan.apply([-40.0, 0.0]);
            pan.tick(|_| true);
            ticks += 1;
            if pan.view()[0] < last {
                break;
            }
            last = pan.view()[0];
        }
        assert!(ticks <= 2, "{ticks}");
    }
    use crate::plugin::CHUNKS_PER_TICK;
    use crate::regions::Regions;
    use crate::scene::{DETAIL, OVERVIEW, Scene};
    use crate::terrain::Terrain;

    fn overview() -> Scene {
        let terrain = Terrain::sample(&DefaultWorldGenerator::default(), &OVERVIEW.window());
        Scene::new(&terrain, 62, OVERVIEW, &Regions::demo())
    }

    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        (0..3).all(|axis| (a[axis] - b[axis]).abs() < 1e-9)
    }

    #[test]
    fn at_rest_the_view_aims_near_the_centre() {
        let direction = look_direction(REST_YAW, REST_PITCH);
        let distance = overview_eye_height() / -direction[1];
        let [x, z] = [
            direction[0] * distance,
            OVERVIEW_Z + direction[2] * distance,
        ];
        assert!(x.hypot(z) < f64::from(OVERVIEW.radius) / 2.0, "{x} {z}");
    }

    #[test]
    fn straight_down_is_off_the_model() {
        assert!(OVERVIEW_Z > f64::from(OVERVIEW.radius));
    }

    #[test]
    fn the_far_rim_is_within_a_render_distance_of_8() {
        let chunks = (OVERVIEW_Z + f64::from(OVERVIEW.radius)) / 16.0;
        assert!(chunks < 8.0);
    }

    #[test]
    fn the_limits_let_the_aim_rest_on_all_the_land() {
        let scene = overview();
        let eye = [0.0, overview_y() + SPECTATOR_EYE_HEIGHT, OVERVIEW_Z];
        for x in -OVERVIEW.radius..OVERVIEW.radius {
            for z in -OVERVIEW.radius..OVERVIEW.radius {
                if let Some(top) = scene.top(x, z) {
                    let (dx, dz) = (f64::from(x) - eye[0], f64::from(z) - eye[2]);
                    let pitch = ((eye[1] - f64::from(top)) / dx.hypot(dz))
                        .atan()
                        .to_degrees() as f32;
                    let yaw = (-dx).atan2(dz).to_degrees() as f32;
                    assert_eq!(resting_look(yaw, pitch), [yaw, pitch], "{x} {z}");
                }
            }
        }
    }

    #[test]
    fn the_land_stands_centred_in_the_frame_at_rest() {
        let height = overview_eye_height();
        let far = (height / (OVERVIEW_Z + LAND_RADIUS)).atan().to_degrees();
        let near = (height / (OVERVIEW_Z - LAND_RADIUS)).atan().to_degrees();
        assert!(((far + near) / 2.0 - f64::from(REST_PITCH)).abs() < 1e-6);
        assert!(near - far < 70.0, "the default field of view");
    }

    #[test]
    fn the_eye_stands_clear_of_the_model() {
        assert!(overview_y() > f64::from(OVERVIEW.max_y) + 6.0);
    }

    #[test]
    fn the_land_ends_inside_its_radius() {
        let scene = overview();
        let reach = (-OVERVIEW.radius..OVERVIEW.radius)
            .flat_map(|x| (-OVERVIEW.radius..OVERVIEW.radius).map(move |z| (x, z)))
            .filter(|(x, z)| scene.top(*x, *z).is_some())
            .map(|(x, z)| f64::from(x).hypot(f64::from(z)))
            .fold(0.0, f64::max);
        assert!(reach <= LAND_RADIUS, "{reach}");
    }

    #[test]
    fn look_directions_match_the_game() {
        assert!(close(look_direction(0.0, 0.0), [0.0, 0.0, 1.0]));
        assert!(close(look_direction(90.0, 0.0), [-1.0, 0.0, 0.0]));
        assert!(close(look_direction(180.0, 0.0), [0.0, 0.0, -1.0]));
        assert!(close(look_direction(0.0, 90.0), [0.0, -1.0, 0.0]));
    }

    const EYE: [f64; 3] = [0.0, 100.0, 0.0];
    const DOWN: [f64; 3] = [0.0, -1.0, 0.0];

    /// A point this many degrees off straight down, 100 blocks below.
    fn at_angle(degrees: f64) -> [f64; 3] {
        [degrees.to_radians().tan() * 100.0, 0.0, 0.0]
    }

    #[test]
    fn the_marker_nearest_the_middle_of_the_screen_wins() {
        let points = [at_angle(3.5), at_angle(1.0), at_angle(-2.0)];
        assert_eq!(aimed(EYE, DOWN, points, None), Some(1));
    }

    #[test]
    fn a_marker_past_the_threshold_is_not_picked_up() {
        assert_eq!(aimed(EYE, DOWN, [at_angle(AIM_ENTER + 0.5)], None), None);
    }

    #[test]
    fn a_height_behind_goes_back_at_the_speed_of_the_zoom() {
        let mut zoom = Zoom::default();
        zoom.tick(false, false);
        assert_eq!(zoom.height_behind(2.0), zoom.height());
        for _ in 0..20 {
            zoom.tick(true, false);
        }
        let before = zoom.height();
        zoom.tick(true, false);
        let step = zoom.height() / before;
        assert!((zoom.height_behind(1.0) - before).abs() < 1e-9);
        assert!((zoom.height_behind(2.0) - before / step).abs() < 1e-9);
    }

    #[test]
    fn height_counts_because_the_angle_is_to_the_icon_itself() {
        let eye = [0.0, 100.0, 60.0];
        let look = look_direction(180.0, 45.0);
        let on_the_ground = [0.0, 40.0, 0.0];
        let raised = [0.0, 70.0, 30.0];
        assert_eq!(aimed(eye, look, [on_the_ground, raised], None), Some(0));
        let off_ground = [0.0, 40.0, 30.0];
        assert_eq!(aimed(eye, look, [off_ground, raised], None), Some(1));
    }

    #[test]
    fn a_marker_holds_on_past_where_it_was_picked_up() {
        assert_eq!(aimed(EYE, DOWN, [at_angle(5.0)], Some(0)), Some(0));
        assert_eq!(aimed(EYE, DOWN, [at_angle(AIM_LEAVE + 0.5)], Some(0)), None);
    }

    #[test]
    fn two_close_markers_do_not_flicker() {
        let points = [at_angle(2.0), at_angle(1.0)];
        assert_eq!(aimed(EYE, DOWN, points, Some(0)), Some(0));
    }

    #[test]
    fn a_clearly_closer_marker_takes_over() {
        let points = [at_angle(3.5), at_angle(1.0)];
        assert_eq!(aimed(EYE, DOWN, points, Some(0)), Some(1));
    }

    #[test]
    fn a_marker_that_is_gone_is_let_go() {
        assert_eq!(aimed(EYE, DOWN, [at_angle(1.0)], Some(4)), Some(0));
    }

    fn settle(look: &mut FreeLook) -> Option<[f32; 2]> {
        (0..SETTLE_TICKS).find_map(|_| look.tick())
    }

    /// Answering a look in motion made it stutter: the client turns between ticks.
    #[test]
    fn a_moving_look_is_never_corrected_however_far_it_goes() {
        let mut look = FreeLook::default();
        for tick in 0..100 {
            look.report(REST_YAW + tick as f32 * 3.0, -40.0 + tick as f32);
            assert_eq!(look.tick(), None);
        }
    }

    #[test]
    fn a_look_inside_its_limits_is_left_alone_at_rest() {
        let mut look = FreeLook::default();
        look.report(REST_YAW + 40.0, 30.0);
        assert_eq!(settle(&mut look), None);
    }

    #[test]
    fn a_look_past_its_limits_drifts_back_in_small_steps_once_at_rest() {
        let mut look = FreeLook::default();
        let (mut yaw, mut pitch) = (REST_YAW + 170.0, -30.0);
        look.report(yaw, pitch);
        assert!(look.is_past());
        let mut step = settle(&mut look).expect("a first step");
        for _ in 0..500 {
            assert!(step[0].hypot(step[1]) <= 2.0 * LOOK_DRIFT_MOST, "{step:?}");
            (yaw, pitch) = (yaw + step[0], pitch + step[1]);
            look.report(yaw, pitch);
            match look.tick() {
                Some(next) => step = next,
                None => break,
            }
        }
        assert!(!look.is_past(), "{yaw} {pitch}");
    }

    #[test]
    fn the_mouse_takes_over_from_a_drift_at_once() {
        let mut look = FreeLook::default();
        look.report(REST_YAW, -30.0);
        let step = settle(&mut look).expect("a first step");
        look.report(REST_YAW + 5.0, -30.0 + step[1]);
        assert_eq!(look.tick(), None);
    }

    /// The answer to a step comes back as a rotation report, and must not pass for the
    /// mouse, or a drift would stop after one step.
    #[test]
    fn the_answer_to_a_step_is_not_the_mouse() {
        let mut look = FreeLook::default();
        look.report(REST_YAW, -30.0);
        let step = settle(&mut look).expect("a first step");
        look.report(REST_YAW + step[0], -30.0 + step[1]);
        assert!(look.tick().is_some());
    }

    #[test]
    fn the_zoom_moves_for_as_long_as_a_key_is_held_and_stays_within_bounds() {
        let mut zoom = Zoom::default();
        let start = zoom.height();
        zoom.tick(true, false);
        zoom.tick(true, false);
        assert!(zoom.height() > start);
        for _ in 0..2000 {
            zoom.tick(true, false);
        }
        assert!((zoom.height() - FARTHEST_ZOOM).abs() < 0.1);
        for _ in 0..2000 {
            zoom.tick(false, true);
        }
        assert!((zoom.height() - CLOSEST_ZOOM).abs() < 0.1);
    }

    #[test]
    fn releasing_the_keys_ends_on_the_height_reached() {
        let mut zoom = Zoom::default();
        zoom.tick(true, false);
        let wanted = zoom.wanted;
        for _ in 0..40 {
            zoom.tick(false, false);
        }
        assert_eq!(zoom.height(), wanted);
    }

    /// Whether a point shows in a window of this shape, at the default field of view.
    fn in_frame(point: [f64; 3], eye: [f64; 3], aspect: f64) -> bool {
        let forward = look_direction(REST_YAW, REST_PITCH);
        let right = [1.0, 0.0, 0.0];
        let up = [
            right[1] * forward[2] - right[2] * forward[1],
            right[2] * forward[0] - right[0] * forward[2],
            right[0] * forward[1] - right[1] * forward[0],
        ];
        let along = |axis: [f64; 3]| -> f64 { (0..3).map(|i| (point[i] - eye[i]) * axis[i]).sum() };
        let depth = along(forward);
        let half_height = 35.0_f64.to_radians().tan();
        depth > 0.0
            && (along(up) / depth).abs() <= half_height
            && (along(right) / depth).abs() <= half_height * aspect
    }

    #[test]
    fn the_whole_model_fits_the_frame_at_rest() {
        let scene = overview();
        let eye = [0.0, overview_y() + SPECTATOR_EYE_HEIGHT, OVERVIEW_Z];
        for x in -OVERVIEW.radius..OVERVIEW.radius {
            for z in -OVERVIEW.radius..OVERVIEW.radius {
                if let Some(top) = scene.top(x, z) {
                    let block = [f64::from(x), f64::from(top), f64::from(z)];
                    assert!(in_frame(block, eye, 1.25), "{block:?}");
                }
            }
        }
    }

    #[test]
    fn the_first_report_is_only_a_reference() {
        let mut mouse = Mouse::default();
        assert_eq!(mouse.report(120.0, 10.0).delta, [0.0, 0.0]);
        assert_eq!(mouse.report(123.0, 8.0).delta, [3.0, -2.0]);
    }

    #[test]
    fn yaw_deltas_take_the_short_way_round() {
        let mut mouse = Mouse::default();
        mouse.report(179.0, 0.0);
        assert_eq!(mouse.report(-179.0, 0.0).delta[0], 2.0);
    }

    #[test]
    fn a_steep_look_is_recentred_once_until_the_client_answers() {
        let mut mouse = Mouse::default();
        mouse.report(0.0, 40.0);
        assert!(mouse.report(0.0, 50.0).recentre);
        assert!(!mouse.report(0.0, 55.0).recentre);
    }

    #[test]
    fn the_answer_to_a_recentring_is_not_mouse_movement() {
        let mut mouse = Mouse::default();
        mouse.report(0.0, 40.0);
        mouse.report(0.0, 50.0);
        assert_eq!(mouse.report(0.0, RECENTRED_PITCH).delta[1], 0.0);
        assert_eq!(mouse.report(0.0, 4.0).delta[1], 4.0);
    }

    /// What a fast mouse did: this used to look like the recentring itself, which
    /// then counted as a jump of 50 degrees when it really came.
    #[test]
    fn a_fast_reversal_during_a_recentring_is_real_movement() {
        let mut mouse = Mouse::default();
        mouse.report(0.0, 40.0);
        mouse.report(0.0, 50.0);
        assert_eq!(mouse.report(0.0, 10.0).delta[1], -40.0);
        assert_eq!(mouse.report(0.0, RECENTRED_PITCH).delta[1], 0.0);
        assert_eq!(mouse.report(0.0, -3.0).delta[1], -3.0);
    }

    #[test]
    fn there_and_back_ends_where_it_started_at_any_speed() {
        let mut mouse = Mouse::default();
        let mut moved = 0.0;
        let mut pitch = 0.0;
        mouse.report(0.0, pitch);
        for step in [60.0_f32, -60.0, 60.0, -60.0, 70.0, -70.0] {
            pitch += step;
            let turn = mouse.report(0.0, pitch);
            moved += turn.delta[1];
            if turn.recentre {
                // The answer lands after one more report, as it does over a network.
                pitch += -step;
                moved += mouse.report(0.0, pitch).delta[1];
                mouse.report(0.0, RECENTRED_PITCH);
                pitch = RECENTRED_PITCH;
                moved += step;
            }
        }
        assert_eq!(moved, 0.0);
    }

    #[test]
    fn a_pan_has_a_top_speed() {
        let mut pan = Pan::default();
        pan.apply([150.0, 0.0]);
        assert_eq!(pan.offset(), [MAX_AIM_LEAD, 0.0]);
    }

    #[test]
    fn a_pan_stays_in_the_chunks_it_streams_at_top_speed() {
        const CHUNKS_RING_PER_CHUNK_MOVED: f64 = 19.0;
        let chunks_per_tick = MAX_PAN_STEP / 16.0 * CHUNKS_RING_PER_CHUNK_MOVED;
        assert!(chunks_per_tick < CHUNKS_PER_TICK as f64);
    }

    #[test]
    fn the_zooms_all_aim_at_the_same_place_from_above_the_terrain() {
        let scene = detail();
        let reach = DETAIL.radius + 16;
        let tallest = (-reach..reach)
            .flat_map(|x| (-reach..reach).map(move |z| (x, z)))
            .filter_map(|(x, z)| scene.top(x, z))
            .max()
            .expect("a model with land");
        assert!(
            CLOSEST_ZOOM + f64::from(SEA_Y) > f64::from(tallest) + 8.0,
            "{tallest}"
        );
    }

    /// A pan is linear: twice the mouse is twice the distance, up to the top speed.
    #[test]
    fn a_pan_has_no_acceleration() {
        let moved = |degrees: f32| {
            let mut pan = Pan::default();
            pan.apply([degrees, 0.0]);
            pan.offset()[0]
        };
        assert_eq!(moved(2.0), 2.0 * moved(1.0));
    }

    #[test]
    fn a_pan_follows_the_mouse() {
        let mut pan = Pan::default();
        pan.apply([2.0, 1.0]);
        let gain = BLOCKS_PER_DEGREE;
        assert_eq!(pan.offset(), [2.0 * gain, gain]);
    }

    fn detail() -> Scene {
        let terrain = Terrain::sample(&DefaultWorldGenerator::default(), &DETAIL.window());
        Scene::new(&terrain, 62, DETAIL, &Regions::demo())
    }

    fn over(scene: &Scene, [x, z]: [f64; 2]) -> bool {
        scene.top(x.floor() as i32, z.floor() as i32).is_some()
    }

    /// The rim is no circle around the origin: it wanders and it is not even centred.
    /// A view must reach it wherever it runs, stretch a little past it and no further.
    #[test]
    fn a_pan_reaches_the_rim_in_every_direction_and_stretches_past_it() {
        let scene = detail();
        for step in 0..32 {
            let angle = f64::from(step) * std::f64::consts::TAU / 32.0;
            let delta = [angle.cos() as f32 * 2.0, angle.sin() as f32 * 2.0];
            let mut pan = Pan::default();
            for _ in 0..1000 {
                pan.apply(delta);
                pan.tick(|at| over(&scene, at));
            }
            let (past, _) = off_model(pan.offset(), &|at| over(&scene, at));
            assert!(past > 0.0, "{angle} stopped short");
            assert!(
                past <= f64::from(PAN_MARGIN),
                "{angle} went too far: {past}"
            );
        }
    }

    /// Runs a pan against a rim at `x = 0`, and gives the steps of the view.
    fn against_the_rim(motion: Motion) -> (Pan, Vec<f64>) {
        let inside = |[x, _]: [f64; 2]| x < 0.0;
        let mut pan = Pan::at([-40.0, 0.0]);
        pan.drive(motion, false);
        let mut steps = Vec::new();
        for _ in 0..200 {
            let before = pan.view()[0];
            pan.apply([1.0, 0.0]);
            pan.tick(inside);
            steps.push(pan.view()[0] - before);
        }
        (pan, steps)
    }

    /// No wall: speed goes down bit by bit, whichever way the view follows.
    #[test]
    fn a_rim_slows_a_pan_down_without_a_jolt() {
        for motion in Motion::ALL {
            let (pan, steps) = against_the_rim(motion);
            let past = pan.view()[0];
            assert!(
                past > 0.0 && past < f64::from(PAN_MARGIN),
                "{motion:?} {past}"
            );
            let top = steps.iter().copied().fold(0.0, f64::max);
            let drop = steps.windows(2).map(|p| p[0] - p[1]).fold(0.0, f64::max);
            assert!(drop < top * 0.2, "{motion:?}: {drop} of {top} in {steps:?}");
            assert!(*steps.last().unwrap() < 0.1, "{motion:?}");
        }
    }

    /// The view used to come back from past the rim once the mouse rested.
    #[test]
    fn a_pan_past_the_rim_stays_where_it_stopped() {
        let inside = |[x, _]: [f64; 2]| x < 0.0;
        for motion in Motion::ALL {
            let (mut pan, _) = against_the_rim(motion);
            let mut furthest = pan.view()[0];
            for _ in 0..100 {
                pan.tick(inside);
                let at = pan.view()[0];
                assert!(at >= furthest - 1e-9, "{motion:?} rolled back");
                furthest = at;
            }
        }
    }

    /// Nothing is stored up past the rim, so that turning round answers at once.
    #[test]
    fn turning_round_at_the_rim_answers_at_once() {
        let inside = |[x, _]: [f64; 2]| x < 0.0;
        for motion in Motion::ALL {
            let (mut pan, _) = against_the_rim(motion);
            let at = pan.view()[0];
            for _ in 0..4 {
                pan.apply([-1.0, 0.0]);
                pan.tick(inside);
            }
            assert!(pan.view()[0] < at - 2.0, "{motion:?} {at} {:?}", pan.view());
        }
    }

    #[test]
    fn a_pan_slides_along_the_rim_instead_of_sticking_to_it() {
        let inside = |[x, _]: [f64; 2]| x < 2.0;
        let mut pan = Pan::default();
        for _ in 0..30 {
            pan.apply([1.0, 3.0]);
            pan.tick(inside);
        }
        assert!(
            pan.view()[1] > 20.0 * 3.0 * BLOCKS_PER_DEGREE,
            "{:?}",
            pan.view()
        );
    }

    fn driven(motion: Motion) -> Pan {
        let mut pan = Pan::default();
        pan.drive(motion, false);
        pan
    }

    /// Small movements add up: a slack or a filter cannot eat a slow mouse.
    #[test]
    fn a_slow_mouse_gets_as_far_in_every_motion() {
        for motion in Motion::ALL {
            let mut pan = driven(motion);
            for _ in 0..2000 {
                pan.apply([0.02, 0.0]);
                pan.tick(|_| true);
            }
            let expected = 2000.0 * 0.02 * BLOCKS_PER_DEGREE;
            assert!(
                pan.view()[0] > expected - 2.0,
                "{motion:?} {:?}",
                pan.view()
            );
            assert!(
                pan.view()[0] < expected + 2.0,
                "{motion:?} {:?}",
                pan.view()
            );
        }
    }

    /// A move ends without ever turning back, on steps too short for a client to overshoot.
    #[test]
    fn a_stop_never_turns_back() {
        for motion in Motion::ALL {
            let mut pan = driven(motion);
            for _ in 0..30 {
                pan.apply([1.6, 0.0]);
                pan.tick(|_| true);
            }
            let mut steps = Vec::new();
            for _ in 0..40 {
                let before = pan.view()[0];
                pan.tick(|_| true);
                steps.push(pan.view()[0] - before);
            }
            assert!(
                steps.iter().all(|step| *step >= -1e-9),
                "{motion:?} {steps:?}"
            );
            let top = steps.iter().copied().fold(0.0, f64::max);
            assert!(top <= 1.6 * BLOCKS_PER_DEGREE + 0.5, "{motion:?} {steps:?}");
            assert!(*steps.last().unwrap() < 0.05, "{motion:?} {steps:?}");
        }
    }

    /// Reports fall unevenly on the ticks: this is what a prediction is for.
    #[test]
    fn a_prediction_evens_out_reports_that_fall_unevenly() {
        let mut pan = driven(Motion::Predicted);
        let mut steps = Vec::new();
        for tick in 0..80 {
            let before = pan.view()[0];
            if tick % 2 == 0 {
                pan.apply([1.0, 0.0]);
                pan.apply([1.0, 0.0]);
            }
            pan.tick(|_| true);
            steps.push(pan.view()[0] - before);
        }
        let settled = &steps[40..];
        let top = settled.iter().copied().fold(0.0, f64::max);
        let least = settled.iter().copied().fold(f64::MAX, f64::min);
        assert!(top - least < 0.5, "{settled:?}");
    }

    #[test]
    fn inertia_is_an_option_that_goes_on_for_a_moment() {
        let travel = |inertia| {
            let mut pan = Pan::default();
            pan.drive(Motion::Damped, inertia);
            for _ in 0..20 {
                pan.apply([1.0, 0.0]);
                pan.tick(|_| true);
            }
            for _ in 0..100 {
                pan.tick(|_| true);
            }
            pan.view()[0]
        };
        assert!(travel(true) > travel(false) + 5.0);
        assert!((travel(false) - 20.0 * BLOCKS_PER_DEGREE).abs() < 0.5);
    }

    /// The client adds the motion of a glide to its target. A view that stops dead
    /// overshoots by tens of blocks; one that ends on a tail of tiny steps cannot.
    #[test]
    fn a_stopping_view_ends_on_steps_too_short_to_overshoot() {
        let mut pan = Pan::default();
        pan.apply([8.0, 0.0]);
        let mut last = 0.0;
        let mut step = f64::MAX;
        for _ in 0..40 {
            pan.tick(|_| true);
            step = pan.view()[0] - last;
            last = pan.view()[0];
        }
        assert!(step.abs() < 0.05, "{step}");
    }

    #[test]
    fn the_detail_pitch_is_what_an_entity_rotation_can_carry() {
        let steps = f64::from(DETAIL_PITCH) * 256.0 / 360.0;
        assert_eq!(steps, steps.round());
    }

    /// The aim is the point on the sea the middle of the screen falls on, so the rim
    /// and the markers line up with what the player sees, at every height.
    #[test]
    fn the_middle_of_the_screen_falls_on_the_pan_offset_at_every_zoom() {
        let look = look_direction(REST_YAW, DETAIL_PITCH);
        for height in [CLOSEST_ZOOM, DEFAULT_ZOOM, COARSE_ABOVE] {
            let eye = [30.0, height, -20.0 + detail_back(height)];
            let distance = height / -look[1];
            assert!((eye[0] + look[0] * distance - 30.0).abs() < 1e-6);
            assert!((eye[2] + look[2] * distance + 20.0).abs() < 1e-6);
        }
    }
}
