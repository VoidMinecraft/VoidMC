//! Where things are in the world. The board lies flat on `x, z` in `0..8`, one
//! block per square, with row 0 at the far side from the seated player.

use crate::game::{SIZE, Square};

pub const BOARD_Y: f64 = 120.0;
/// Yaw that faces the far side of the board, and the pitch that centres it.
pub const YAW: f32 = 180.0;
pub const PITCH: f32 = 45.0;

const EYE: [f64; 3] = [SIZE as f64 / 2.0, BOARD_Y + 7.0, SIZE as f64 + 3.0];
/// A seated player's feet sit 0.6 below the vehicle and the eyes 1.62 above the feet.
const SEAT_TO_EYE: f64 = 1.62 - 0.6;
/// How far past the edge a held piece can follow the crosshair.
const MARGIN: f64 = 0.5;

/// Position of the invisible vehicle that holds the player's eye at [`EYE`].
pub fn seat() -> [f64; 3] {
    [EYE[0], EYE[1] - SEAT_TO_EYE, EYE[2]]
}

/// The point of the board plane under the crosshair; `None` above the horizon.
pub fn aim(yaw: f32, pitch: f32) -> Option<(f64, f64)> {
    let (yaw, pitch) = (f64::from(yaw).to_radians(), f64::from(pitch).to_radians());
    let (dx, dy, dz) = (
        -yaw.sin() * pitch.cos(),
        -pitch.sin(),
        yaw.cos() * pitch.cos(),
    );
    if dy >= 0.0 {
        return None;
    }
    let distance = (BOARD_Y - EYE[1]) / dy;
    Some((EYE[0] + dx * distance, EYE[2] + dz * distance))
}

/// Keeps a held piece near the board when the crosshair leaves it.
pub fn clamp(x: f64, z: f64) -> (f64, f64) {
    let limit = SIZE as f64 + MARGIN;
    (x.clamp(-MARGIN, limit), z.clamp(-MARGIN, limit))
}

/// The playable square at this point, if any.
pub fn square_at(x: f64, z: f64) -> Option<Square> {
    if x < 0.0 || z < 0.0 {
        return None;
    }
    Square::new(z.floor() as u8, x.floor() as u8)
}

pub fn centre(square: Square) -> (f64, f64) {
    (f64::from(square.col()) + 0.5, f64::from(square.row()) + 0.5)
}

/// A point along a jump from `from` to `to`, `t` going from 0 to 1.
pub fn arc(from: [f64; 3], to: [f64; 3], t: f64, height: f64) -> [f64; 3] {
    let eased = t * t * (3.0 - 2.0 * t);
    let lerp = |a: f64, b: f64| a + (b - a) * eased;
    [
        lerp(from[0], to[0]),
        lerp(from[1], to[1]) + height * 4.0 * t * (1.0 - t),
        lerp(from[2], to[2]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn the_initial_view_looks_at_the_middle_of_the_board() {
        let (x, z) = aim(YAW, PITCH).unwrap();
        assert!(close(x, 4.0));
        assert!(close(z, 4.0));
    }

    #[test]
    fn turning_right_moves_the_aim_towards_positive_x() {
        let (x, _) = aim(YAW + 10.0, PITCH).unwrap();
        assert!(x > 4.0);
    }

    #[test]
    fn looking_down_further_comes_closer_to_the_player() {
        let (_, z) = aim(YAW, PITCH + 10.0).unwrap();
        assert!(z > 4.0);
    }

    #[test]
    fn looking_at_the_sky_aims_at_nothing() {
        assert_eq!(aim(YAW, 0.0), None);
        assert_eq!(aim(YAW, -30.0), None);
    }

    #[test]
    fn points_map_to_playable_squares_only() {
        assert_eq!(square_at(1.5, 0.5), Square::new(0, 1));
        assert_eq!(square_at(0.5, 0.5), None);
        assert_eq!(square_at(-0.5, 0.5), None);
        assert_eq!(square_at(8.5, 1.5), None);
    }

    #[test]
    fn a_square_contains_its_centre() {
        let square = Square::new(5, 2).unwrap();
        let (x, z) = centre(square);
        assert_eq!(square_at(x, z), Some(square));
    }

    #[test]
    fn a_held_piece_stays_near_the_board() {
        assert_eq!(clamp(-20.0, 50.0), (-0.5, 8.5));
        assert_eq!(clamp(3.0, 4.0), (3.0, 4.0));
    }

    #[test]
    fn an_arc_starts_and_ends_on_its_points_and_rises_between() {
        let (from, to) = ([0.0, 1.0, 0.0], [2.0, 1.0, 2.0]);
        assert_eq!(arc(from, to, 0.0, 1.0), from);
        assert_eq!(arc(from, to, 1.0, 1.0), to);
        let middle = arc(from, to, 0.5, 1.0);
        assert!(close(middle[0], 1.0));
        assert!(close(middle[1], 2.0));
    }
}
