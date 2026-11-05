//! What the player aims at, from where, and how the mouse moves it.
//!
//! The overview stands south of the model and well above it. Aiming straight down
//! is then off the model, so no one needs the angle where a look spins on itself.

use crate::scene::SEA_Y;

/// How steeply the overview looks down. Everything about it follows from this.
pub const REST_PITCH: f32 = 55.0;
/// North.
pub const REST_YAW: f32 = 180.0;
/// South of the model's centre, past its rim: aiming straight down is off the land.
pub const OVERVIEW_Z: f64 = 65.0;
/// Where the land ends, whatever the rim does.
const LAND_RADIUS: f64 = 54.0;
/// Where a look can land, around this point of the model. Things stand up, so a look at
/// the far rim lands past it, while the near rim needs no such room.
const STOP_CENTRE_Z: f64 = -5.0;
const STOP_RADIUS: f64 = 52.0;
/// A look held on the stop must count as inside it, or holding it would never end.
const STOP_SLACK: f64 = 0.01;
pub const SPECTATOR_EYE_HEIGHT: f64 = 1.62;

/// The detail view is close to straight down and low, because a far look runs out of
/// what a render distance of 8 shows.
/// An entity's rotation travels as 1/256 of a turn: this is what the camera really has.
pub const DETAIL_PITCH: f32 = 56.0 * 360.0 / 256.0;
/// Heights of the detail view above the sea, from close to far. The first clears the
/// tallest column of the model. Past the last one the fog that ends a render distance
/// of 8 darkens the whole view.
pub const ZOOMS: [f64; 7] = [40.0, 50.0, 60.0, 72.0, 84.0, 95.0, 105.0];
pub const DEFAULT_ZOOM: usize = 5;

/// The flattest look a stop ever works out, whose landing is still a long way off.
const FLATTEST_PITCH: f32 = 1.0;

/// Past this pitch a rider is turned back, well before the ±90° stop that would
/// swallow mouse movement.
const RECENTRE_PITCH: f32 = 45.0;
/// Where a rider is turned back to. The client answers at once with exactly this.
pub const RECENTRED_PITCH: f32 = 0.0;
/// How fast the mouse pans, the same at any speed.
const BLOCKS_PER_DEGREE: f64 = 8.0;
/// As far as a report can move the view: chunks can follow, and the model stays whole.
pub const MAX_PAN_STEP: f64 = 40.0;

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

pub fn detail_y(zoom: usize) -> f64 {
    f64::from(SEA_Y) + ZOOMS[zoom]
}

/// How far south of the point it aims at the detail view stands.
pub fn detail_back(zoom: usize) -> f64 {
    ZOOMS[zoom] / f64::from(DETAIL_PITCH).to_radians().tan()
}

fn shortest_turn(from: f32, to: f32) -> f32 {
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

/// The marker aimed at, by its angle from the middle of the screen. The one already
/// aimed at holds on for a while, so that the choice never flickers between two.
pub fn aimed(
    eye: [f64; 3],
    direction: [f64; 3],
    points: impl IntoIterator<Item = [f64; 3]>,
    current: Option<usize>,
) -> Option<usize> {
    let angle = |point: [f64; 3]| {
        let to_point: [f64; 3] = std::array::from_fn(|axis| point[axis] - eye[axis]);
        let length = to_point.iter().map(|v| v * v).sum::<f64>().sqrt();
        let along: f64 = (0..3).map(|axis| to_point[axis] * direction[axis]).sum();
        (along / length.max(f64::EPSILON))
            .clamp(-1.0, 1.0)
            .acos()
            .to_degrees()
    };
    let angles: Vec<f64> = points.into_iter().map(angle).collect();
    let nearest = (0..angles.len()).min_by(|a, b| angles[*a].total_cmp(&angles[*b]));
    let held = current.filter(|index| angles.get(*index).is_some_and(|a| *a <= AIM_LEAVE));
    let picked = nearest.filter(|index| angles[*index] <= AIM_ENTER);
    match (held, picked) {
        (Some(held), Some(picked)) if angles[picked] + AIM_SWITCH < angles[held] => Some(picked),
        (Some(held), _) => Some(held),
        (None, picked) => picked,
    }
}

/// Where a free look meets the sea, from the model's centre.
fn landing(yaw: f32, pitch: f32) -> [f64; 2] {
    let direction = look_direction(yaw, pitch.max(FLATTEST_PITCH));
    let distance = overview_eye_height() / -direction[1];
    [
        direction[0] * distance,
        OVERVIEW_Z + direction[2] * distance,
    ]
}

/// The absolute `[yaw, pitch]` that brings a free look back on the land, for the axes
/// that change. The stop is the land's own shape: a look past it is pulled back to the
/// closest point of the rim, so the middle of the screen never leaves the island.
/// Reports can be late, so answering one must not depend on what was answered before:
/// an absolute angle stays right if sent twice.
pub fn stop(yaw: f32, pitch: f32) -> [Option<f32>; 2] {
    let [x, z] = landing(yaw, pitch);
    let from_centre = z - STOP_CENTRE_Z;
    let reach = x.hypot(from_centre);
    if reach <= STOP_RADIUS + STOP_SLACK && pitch >= FLATTEST_PITCH {
        return [None, None];
    }
    let pull = (STOP_RADIUS / reach).min(1.0);
    let (x, z) = (x * pull, from_centre * pull + STOP_CENTRE_Z - OVERVIEW_Z);
    let held_yaw = (-x).atan2(z).to_degrees() as f32;
    let held_pitch = overview_eye_height().atan2(x.hypot(z)).to_degrees() as f32;
    [Some(held_yaw), Some(held_pitch)]
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

/// Where the mouse has moved a panning view, in blocks from its resting place.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pan([f64; 2]);

impl Pan {
    pub fn at(offset: [f64; 2]) -> Self {
        Self(offset)
    }

    /// The view looks north: turning right is east, looking down is south.
    /// Moves the view axis by axis and only over what `on_model` accepts, so that the
    /// aim follows the real rim, wherever it runs, and slides along it.
    pub fn apply(&mut self, delta: [f32; 2], on_model: impl Fn([f64; 2]) -> bool) {
        let step = delta.map(|degrees| f64::from(degrees) * BLOCKS_PER_DEGREE);
        let length = step[0].hypot(step[1]);
        let slowed = (MAX_PAN_STEP / length.max(f64::EPSILON)).min(1.0);
        for (axis, step) in step.into_iter().enumerate() {
            let step = step * slowed;
            let moves = step.abs().ceil().max(1.0) as u32;
            for _ in 0..moves {
                let mut next = self.0;
                next[axis] += step / f64::from(moves);
                if !on_model(next) {
                    break;
                }
                self.0 = next;
            }
        }
    }

    pub fn offset(self) -> [f64; 2] {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use voidmc::DefaultWorldGenerator;

    use super::*;
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
        let [x, z] = landing(REST_YAW, REST_PITCH);
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
    fn the_stops_let_the_aim_reach_all_the_land() {
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
                    let [held_yaw, held_pitch] = stop(yaw, pitch);
                    let off_yaw = held_yaw.map_or(0.0, |held| shortest_turn(yaw, held));
                    let off_pitch = held_pitch.map_or(0.0, |held| held - pitch);
                    assert!(off_yaw.hypot(off_pitch) < AIM_ENTER as f32, "{x} {z}");
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

    #[test]
    fn inside_the_stops_a_look_stays_free() {
        assert_eq!(stop(REST_YAW + 10.0, REST_PITCH), [None, None]);
        assert_eq!(stop(-170.0, 50.0), [None, None]);
    }

    #[test]
    fn rest_is_inside_the_stops() {
        assert_eq!(stop(REST_YAW, REST_PITCH), [None, None]);
    }

    /// The view used to leave the island at both ends of the pitch and of the yaw:
    /// rectangular stops knew nothing of the land, so a look could end on the black sky.
    #[test]
    fn no_look_ends_off_the_land() {
        for yaw in (0..360).step_by(5) {
            for pitch in (-90..=90).step_by(5) {
                let (yaw, pitch) = (yaw as f32, pitch as f32);
                let [held_yaw, held_pitch] = stop(yaw, pitch);
                let (yaw, pitch) = (held_yaw.unwrap_or(yaw), held_pitch.unwrap_or(pitch));
                let [x, z] = landing(yaw, pitch);
                let reach = x.hypot(z - STOP_CENTRE_Z);
                assert!(reach <= STOP_RADIUS + STOP_SLACK, "{yaw} {pitch}: {x} {z}");
                assert!(pitch > FLATTEST_PITCH, "{yaw} {pitch}");
            }
        }
    }

    /// A held look that still counted as past the stop was answered at every report,
    /// and each answer swallowed the mouse: the view froze on the rim.
    #[test]
    fn a_look_held_on_the_stop_is_left_alone() {
        for yaw in (0..360).step_by(7) {
            for pitch in (-90..=90).step_by(7) {
                if let [Some(yaw), Some(pitch)] = stop(yaw as f32, pitch as f32) {
                    assert_eq!(stop(yaw, pitch), [None, None], "{yaw} {pitch}");
                }
            }
        }
    }

    #[test]
    fn a_stop_slides_along_the_rim_instead_of_centring() {
        let [yaw, _] = stop(REST_YAW + 70.0, REST_PITCH).map(Option::unwrap);
        assert!(shortest_turn(REST_YAW, yaw) > 30.0, "{yaw}");
    }

    #[test]
    fn straight_down_and_straight_up_stay_on_the_land() {
        assert!(stop(REST_YAW, 90.0)[1].is_some());
        assert!(stop(REST_YAW, -90.0)[1].is_some());
    }

    #[test]
    fn a_burst_of_late_reports_asks_for_the_same_angle() {
        let answers: Vec<_> = (0..5).map(|_| stop(REST_YAW, 5.0)).collect();
        assert!(answers.iter().all(|a| *a == answers[0]));
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
        pan.apply([150.0, 0.0], |_| true);
        assert_eq!(pan.offset(), [MAX_PAN_STEP, 0.0]);
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
        assert!(detail_y(0) > f64::from(tallest) + 8.0, "{tallest}");
        assert!(detail_back(0) > 0.0);
        assert!(ZOOMS.windows(2).all(|pair| pair[0] < pair[1]));
    }

    /// A pan is linear: twice the mouse is twice the distance, up to the top speed.
    #[test]
    fn a_pan_has_no_acceleration() {
        let moved = |degrees: f32| {
            let mut pan = Pan::default();
            pan.apply([degrees, 0.0], |_| true);
            pan.offset()[0]
        };
        assert_eq!(moved(2.0), 2.0 * moved(1.0));
    }

    #[test]
    fn a_pan_follows_the_mouse() {
        let mut pan = Pan::default();
        pan.apply([2.0, 1.0], |_| true);
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
    /// A view must reach it wherever it runs, and never go past it.
    #[test]
    fn a_pan_reaches_the_rim_in_every_direction_and_stops_there() {
        let scene = detail();
        for step in 0..32 {
            let angle = f64::from(step) * std::f64::consts::TAU / 32.0;
            let delta = [angle.cos() as f32 * 2.0, angle.sin() as f32 * 2.0];
            let mut pan = Pan::default();
            for _ in 0..1000 {
                pan.apply(delta, |at| over(&scene, at));
                assert!(over(&scene, pan.offset()), "{angle} went off the model");
            }
            let [x, z] = pan.offset();
            let on_the_rim = [[2.0, 0.0], [-2.0, 0.0], [0.0, 2.0], [0.0, -2.0]]
                .iter()
                .any(|[dx, dz]| !over(&scene, [x + dx, z + dz]));
            assert!(on_the_rim, "{angle} stopped short at {x} {z}");
        }
    }

    #[test]
    fn a_pan_slides_along_the_rim_instead_of_sticking_to_it() {
        let mut pan = Pan::default();
        pan.apply([1.0, 3.0], |[x, _]| x < 2.0);
        let [x, z] = pan.offset();
        assert!(x < 2.0 && z == 3.0 * BLOCKS_PER_DEGREE, "{x} {z}");
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
        for (zoom, height) in ZOOMS.iter().enumerate() {
            let eye = [30.0, detail_y(zoom), -20.0 + detail_back(zoom)];
            let distance = height / -look[1];
            assert!((eye[0] + look[0] * distance - 30.0).abs() < 1e-6);
            assert!((eye[2] + look[2] * distance + 20.0).abs() < 1e-6);
        }
    }
}
