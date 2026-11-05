//! What the player aims at, from where, and how the mouse moves it.
//!
//! The view stands south of the model and well above it. Aiming straight down is
//! then off the model, so no one needs the angle where a look spins on itself.

use crate::scene::{MAX_Y, RADIUS};

/// South of the model's centre, past its rim.
pub const CAMERA_Z: f64 = 62.0;
/// A client draws 8 chunks around the camera at a render distance of 8.
pub const CAMERA_Y: f64 = 154.0;
/// North.
pub const REST_YAW: f32 = 180.0;
/// Aims at the model's centre.
pub const REST_PITCH: f32 = 55.4;

/// Nothing on the model can reach the eye, whatever the terrain.
const _: () = assert!(CAMERA_Y > MAX_Y as f64 + 8.0);

const YAW_LIMIT: f32 = 38.0;
/// From the far rim, which is a shallow look, to the near one, which is nearly
/// straight down. Rest is between the two.
const PITCH_LIMITS: [f32; 2] = [37.0, 86.0];

/// Past this pitch a rider is turned back, well before the ±90° stop that would
/// swallow mouse movement.
const RECENTRE_PITCH: f32 = 45.0;
const BLOCKS_PER_DEGREE: f64 = 2.0;
const VIEW_LIMIT: f64 = RADIUS as f64;

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

/// The point closest to a ray, among those within `max_angle` degrees of it.
pub fn nearest_to_ray(
    eye: [f64; 3],
    direction: [f64; 3],
    points: impl IntoIterator<Item = [f64; 3]>,
    max_angle: f64,
) -> Option<usize> {
    let threshold = max_angle.to_radians().cos();
    let cosine = |point: [f64; 3]| {
        let to_point: [f64; 3] = std::array::from_fn(|axis| point[axis] - eye[axis]);
        let length = to_point.iter().map(|v| v * v).sum::<f64>().sqrt();
        let dot: f64 = (0..3).map(|axis| to_point[axis] * direction[axis]).sum();
        dot / length
    };
    points
        .into_iter()
        .map(cosine)
        .enumerate()
        .filter(|(_, cosine)| *cosine >= threshold)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

/// The absolute `[yaw, pitch]` that brings a free look back on the model, for the
/// axes past a stop. Reports can be late, so answering one must not depend on
/// what was answered before: an absolute angle stays right if sent twice.
pub fn stop(yaw: f32, pitch: f32) -> [Option<f32>; 2] {
    let off_yaw = shortest_turn(REST_YAW, yaw);
    let held_yaw = off_yaw.clamp(-YAW_LIMIT, YAW_LIMIT);
    let held_pitch = pitch.clamp(PITCH_LIMITS[0], PITCH_LIMITS[1]);
    [
        (held_yaw != off_yaw).then_some(REST_YAW + held_yaw),
        (held_pitch != pitch).then_some(held_pitch),
    ]
}

/// What one rotation report amounts to.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Turn {
    /// Degrees turned since the last report, as `[yaw, pitch]`.
    pub delta: [f32; 2],
    /// Pitch to add to the player, relatively, to bring them back.
    pub recentre: Option<f32>,
}

/// Follows the rotation a rider reports, whose look is never shown.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Mouse {
    last: Option<[f32; 2]>,
    /// A recentring the client has not reported yet. Reports carry no marker, but
    /// it is far larger than a tick of mouse movement, so it shows.
    unseen: Option<f32>,
}

impl Mouse {
    pub fn report(&mut self, yaw: f32, pitch: f32) -> Turn {
        let last = self.last.replace([yaw, pitch]);
        let mut delta = [0.0; 2];
        if let Some([last_yaw, last_pitch]) = last {
            delta = [shortest_turn(last_yaw, yaw), pitch - last_pitch];
            if let Some(shift) = self.unseen
                && (delta[1] - shift).abs() < delta[1].abs()
            {
                delta[1] -= shift;
                self.unseen = None;
            }
        }
        let recentre = (self.unseen.is_none() && pitch.abs() > RECENTRE_PITCH).then_some(-pitch);
        self.unseen = self.unseen.or(recentre);
        Turn { delta, recentre }
    }
}

/// Where the mouse has moved a panning view, in blocks from its resting place.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pan([f64; 2]);

impl Pan {
    /// The view looks north: turning right is east, looking down is south.
    pub fn apply(&mut self, delta: [f32; 2]) {
        for (offset, degrees) in self.0.iter_mut().zip(delta) {
            *offset += f64::from(degrees) * BLOCKS_PER_DEGREE;
        }
        let distance = self.0[0].hypot(self.0[1]);
        if distance > VIEW_LIMIT {
            self.0 = self.0.map(|v| v * VIEW_LIMIT / distance);
        }
    }

    pub fn offset(self) -> [f64; 2] {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use voidmc::{BiomeId, DefaultWorldGenerator};

    use super::*;
    use crate::scene::{SEA_Y, Scene, window};
    use crate::terrain::Terrain;

    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        (0..3).all(|axis| (a[axis] - b[axis]).abs() < 1e-9)
    }

    /// Where a look from the resting place meets the sea, from the model's centre.
    fn landing(yaw: f32, pitch: f32) -> [f64; 2] {
        let direction = look_direction(yaw, pitch);
        let distance = (CAMERA_Y - f64::from(SEA_Y)) / -direction[1];
        [direction[0] * distance, CAMERA_Z + direction[2] * distance]
    }

    #[test]
    fn at_rest_the_view_aims_at_the_centre() {
        let [x, z] = landing(REST_YAW, REST_PITCH);
        assert!(x.abs() < 1.0 && z.abs() < 1.0, "{x} {z}");
    }

    #[test]
    fn straight_down_is_off_the_model() {
        assert!(CAMERA_Z > f64::from(RADIUS));
    }

    #[test]
    fn the_far_rim_is_within_a_render_distance_of_8() {
        let chunks = (CAMERA_Z + f64::from(RADIUS)) / 16.0;
        assert!(chunks < 8.0);
    }

    #[test]
    fn the_stops_keep_the_aim_near_the_model() {
        for (yaw, pitch) in [
            (REST_YAW, PITCH_LIMITS[0]),
            (REST_YAW, PITCH_LIMITS[1]),
            (REST_YAW + YAW_LIMIT, REST_PITCH),
            (REST_YAW - YAW_LIMIT, REST_PITCH),
        ] {
            let [x, z] = landing(yaw, pitch);
            assert!(x.hypot(z) < f64::from(RADIUS) * 1.1, "{yaw} {pitch}");
        }
    }

    #[test]
    fn look_directions_match_the_game() {
        assert!(close(look_direction(0.0, 0.0), [0.0, 0.0, 1.0]));
        assert!(close(look_direction(90.0, 0.0), [-1.0, 0.0, 0.0]));
        assert!(close(look_direction(180.0, 0.0), [0.0, 0.0, -1.0]));
        assert!(close(look_direction(0.0, 90.0), [0.0, -1.0, 0.0]));
    }

    #[test]
    fn the_point_nearest_the_ray_wins_within_the_angle() {
        let eye = [0.0, 100.0, 0.0];
        let down = [0.0, -1.0, 0.0];
        let points = [[30.0, 0.0, 0.0], [5.0, 0.0, 0.0], [-3.0, 0.0, 3.0]];
        assert_eq!(nearest_to_ray(eye, down, points, 5.0), Some(2));
        assert_eq!(nearest_to_ray(eye, down, [[30.0, 0.0, 0.0]], 5.0), None);
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

    #[test]
    fn a_stop_only_holds_the_axis_past_it() {
        assert_eq!(stop(REST_YAW, 90.0), [None, Some(PITCH_LIMITS[1])]);
        assert_eq!(stop(REST_YAW, 10.0), [None, Some(PITCH_LIMITS[0])]);
        let past = REST_YAW + YAW_LIMIT + 4.0;
        assert_eq!(stop(past, 60.0), [Some(REST_YAW + YAW_LIMIT), None]);
    }

    #[test]
    fn a_stop_takes_the_short_way_round() {
        assert_eq!(stop(-138.0, 60.0), [Some(REST_YAW + YAW_LIMIT), None]);
    }

    #[test]
    fn a_burst_of_late_reports_asks_for_the_same_angle() {
        let answers: Vec<_> = (0..5).map(|_| stop(REST_YAW, 20.0)).collect();
        assert!(answers.iter().all(|a| *a == [None, Some(PITCH_LIMITS[0])]));
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
        let scene = Scene::new(
            &Terrain::sample(&DefaultWorldGenerator::default(), &window()),
            62,
        );
        let eye = [0.0, CAMERA_Y + 1.62, CAMERA_Z];
        for x in -RADIUS..RADIUS {
            for z in -RADIUS..RADIUS {
                if let Some(top) = scene.top(x, z) {
                    let block = [f64::from(x), f64::from(top), f64::from(z)];
                    assert!(in_frame(block, eye, 1.25), "{block:?}");
                }
            }
        }
    }

    #[test]
    fn the_eye_clears_the_highest_column() {
        let scene = Scene::new(
            &Terrain::sample(&DefaultWorldGenerator::default(), &window()),
            62,
        );
        let highest = (-RADIUS..RADIUS)
            .flat_map(|x| (-RADIUS..RADIUS).map(move |z| (x, z)))
            .filter_map(|(x, z)| scene.top(x, z))
            .max();
        assert!(f64::from(highest.unwrap()) + 8.0 < CAMERA_Y);
    }

    #[test]
    fn a_pan_stays_in_the_chunks_the_client_has() {
        let scene = Scene::new(
            &Terrain::sample(&DefaultWorldGenerator::default(), &window()),
            62,
        );
        let chunks: HashSet<_> = scene
            .chunks(BiomeId::plains(), 64)
            .iter()
            .map(|c| (c.x, c.z))
            .collect();
        for delta in [[1e4, 0.0], [-1e4, 0.0], [0.0, 1e4], [0.0, -1e4], [1e4, 1e4]] {
            let mut pan = Pan::default();
            pan.apply(delta);
            let [x, z] = pan.offset();
            let chunk = (
                (x / 16.0).floor() as i32,
                ((z + CAMERA_Z) / 16.0).floor() as i32,
            );
            assert!(chunks.contains(&chunk), "{chunk:?}");
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
    fn a_steep_look_is_recentred_once_until_the_client_reports_it() {
        let mut mouse = Mouse::default();
        mouse.report(0.0, 40.0);
        assert_eq!(mouse.report(0.0, 50.0).recentre, Some(-50.0));
        assert_eq!(mouse.report(0.0, 55.0).recentre, None);
    }

    #[test]
    fn recentring_is_not_mouse_movement_and_loses_none() {
        let mut mouse = Mouse::default();
        mouse.report(0.0, 40.0);
        mouse.report(0.0, 50.0);
        // The client applied -50 and the mouse went 3 further down meanwhile.
        assert_eq!(mouse.report(0.0, 3.0).delta[1], 3.0);
        assert_eq!(mouse.report(0.0, 4.0).delta[1], 1.0);
    }

    #[test]
    fn a_report_sent_before_the_recentring_still_counts() {
        let mut mouse = Mouse::default();
        mouse.report(0.0, 40.0);
        mouse.report(0.0, 50.0);
        assert_eq!(mouse.report(0.0, 52.0).delta[1], 2.0);
        assert_eq!(mouse.report(0.0, 2.0).delta[1], 0.0);
    }

    #[test]
    fn a_pan_follows_the_mouse() {
        let mut pan = Pan::default();
        pan.apply([5.0, 2.0]);
        assert_eq!(pan.offset(), [10.0, 4.0]);
    }

    #[test]
    fn a_pan_stays_over_the_model() {
        let mut pan = Pan::default();
        pan.apply([10_000.0, 10_000.0]);
        let [x, z] = pan.offset();
        assert!((x.hypot(z) - VIEW_LIMIT).abs() < 1e-6);
    }
}
