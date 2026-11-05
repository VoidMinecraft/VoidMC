//! Mouse movement read from the player's rotation, as a cursor on the model.
//!
//! Only rotation deltas count. The player's real look is never shown, so it can
//! be turned back at will, and the view has no angle where it misbehaves.

use crate::scene::RADIUS;

/// Past this pitch the player is turned back, well before the ±90° stop that
/// would swallow mouse movement.
const RECENTRE_PITCH: f32 = 45.0;
/// Cursor speed. About what the same mouse movement does to a look at the model.
const BLOCKS_PER_DEGREE: f64 = 2.0;
/// How far the cursor goes from the view's centre before pushing the view.
const VIEW_EDGE: [f64; 2] = [62.0, 42.0];
const CURSOR_LIMIT: f64 = RADIUS as f64 - 2.0;

fn shortest_turn(from: f32, to: f32) -> f32 {
    (to - from + 180.0).rem_euclid(360.0) - 180.0
}

/// What one rotation report amounts to.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Turn {
    /// Degrees turned since the last report, as `[yaw, pitch]`.
    pub delta: [f32; 2],
    /// Pitch to add to the player, relatively, to bring them back.
    pub recentre: Option<f32>,
}

/// Follows the rotation a client reports.
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

/// The cursor and the centre of the view, in blocks from the model's centre.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pointer {
    pub cursor: [f64; 2],
    pub view: [f64; 2],
}

impl Pointer {
    /// The camera looks north: turning right is east, looking down is south.
    pub fn apply(&mut self, delta: [f32; 2]) {
        for axis in 0..2 {
            self.cursor[axis] += f64::from(delta[axis]) * BLOCKS_PER_DEGREE;
        }
        let distance = self.cursor[0].hypot(self.cursor[1]);
        if distance > CURSOR_LIMIT {
            self.cursor = self.cursor.map(|v| v * CURSOR_LIMIT / distance);
        }
        for axis in 0..2 {
            let edge = VIEW_EDGE[axis];
            self.view[axis] =
                self.view[axis].clamp(self.cursor[axis] - edge, self.cursor[axis] + edge);
        }
    }

    /// The point nearest the cursor, among those within `radius` blocks of it.
    pub fn nearest(
        &self,
        points: impl IntoIterator<Item = [f64; 2]>,
        radius: f64,
    ) -> Option<usize> {
        let distance = |p: [f64; 2]| (p[0] - self.cursor[0]).hypot(p[1] - self.cursor[1]);
        points
            .into_iter()
            .map(distance)
            .enumerate()
            .filter(|(_, d)| *d <= radius)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(index, _)| index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn looking_up_recentres_too() {
        let mut mouse = Mouse::default();
        mouse.report(0.0, -40.0);
        assert_eq!(mouse.report(0.0, -46.0).recentre, Some(46.0));
    }

    #[test]
    fn the_cursor_follows_the_mouse_on_screen() {
        let mut pointer = Pointer::default();
        pointer.apply([5.0, 2.0]);
        assert_eq!(pointer.cursor, [10.0, 4.0]);
        assert_eq!(pointer.view, [0.0, 0.0]);
    }

    #[test]
    fn the_cursor_stays_on_the_model() {
        let mut pointer = Pointer::default();
        pointer.apply([10_000.0, 10_000.0]);
        let distance = pointer.cursor[0].hypot(pointer.cursor[1]);
        assert!((distance - CURSOR_LIMIT).abs() < 1e-6);
    }

    #[test]
    fn the_cursor_pushes_the_view_past_its_edge() {
        let mut pointer = Pointer::default();
        pointer.apply([40.0, 0.0]);
        assert_eq!(pointer.cursor[0], 80.0);
        assert_eq!(pointer.view[0], 80.0 - VIEW_EDGE[0]);
        pointer.apply([-5.0, 0.0]);
        assert_eq!(pointer.view[0], 80.0 - VIEW_EDGE[0]);
    }

    #[test]
    fn the_view_stops_when_the_cursor_does() {
        let mut pointer = Pointer::default();
        pointer.apply([0.0, -10_000.0]);
        assert_eq!(pointer.view[1], -(CURSOR_LIMIT - VIEW_EDGE[1]));
    }

    #[test]
    fn the_nearest_point_within_the_radius_wins() {
        let pointer = Pointer::default();
        let points = [[30.0, 0.0], [5.0, 0.0], [0.0, 3.0]];
        assert_eq!(pointer.nearest(points, 10.0), Some(2));
        assert_eq!(pointer.nearest([[30.0, 0.0]], 10.0), None);
    }
}
