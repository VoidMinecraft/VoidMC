//! The selector a player holds open with sprint: the mouse turns it between what can be
//! changed of the map, and steps through the choices of each.

/// How far the mouse has to go for a step, in degrees of the view.
const STEP_DEGREES: f64 = 4.0;
/// A movement this much stronger on one axis is meant for that axis.
const CLEAR_LEAD: f64 = 1.5;

pub const CATEGORIES: usize = 4;

/// Sits on the screen of a player while they hold the key.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Wheel {
    /// Which sector is on, clockwise from the top.
    pub category: usize,
    /// How far the mouse went since the last step.
    pointer: [f64; 2],
    /// Choices to step through, which the map has not caught up with.
    pending: i32,
}

impl Wheel {
    /// Across turns the wheel, and up and down steps through its sector.
    pub fn turn(&mut self, [yaw, pitch]: [f32; 2]) {
        self.pointer[0] += f64::from(yaw);
        self.pointer[1] += f64::from(pitch);
        let [across, down] = self.pointer;
        if across.abs() >= STEP_DEGREES && across.abs() > down.abs() * CLEAR_LEAD {
            let turned = if across > 0.0 { 1 } else { CATEGORIES - 1 };
            self.category = (self.category + turned) % CATEGORIES;
            self.pending = 0;
            self.pointer = [0.0; 2];
        } else if down.abs() >= STEP_DEGREES && down.abs() > across.abs() * CLEAR_LEAD {
            self.pending += if down > 0.0 { 1 } else { -1 };
            self.pointer = [0.0; 2];
        }
    }

    /// The steps through the choices since this was last asked.
    pub fn take_steps(&mut self) -> i32 {
        std::mem::take(&mut self.pending)
    }
}

/// Where stepping from a choice leads, going round.
pub fn stepped(index: usize, steps: i32, count: usize) -> usize {
    (index as i32 + steps).rem_euclid(count as i32) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn across_turns_the_wheel_both_ways() {
        let mut wheel = Wheel::default();
        wheel.turn([5.0, 0.0]);
        assert_eq!(wheel.category, 1);
        wheel.turn([-5.0, 0.0]);
        wheel.turn([-5.0, 0.0]);
        assert_eq!(wheel.category, CATEGORIES - 1);
    }

    #[test]
    fn small_movements_add_up_to_a_step() {
        let mut wheel = Wheel::default();
        for _ in 0..3 {
            wheel.turn([0.0, 1.0]);
            assert_eq!(wheel.pending, 0);
        }
        wheel.turn([0.0, 1.0]);
        assert_eq!(wheel.take_steps(), 1);
        assert_eq!(wheel.take_steps(), 0);
    }

    #[test]
    fn a_diagonal_says_nothing() {
        let mut wheel = Wheel::default();
        wheel.turn([5.0, 5.0]);
        assert_eq!((wheel.category, wheel.pending), (0, 0));
    }

    #[test]
    fn choices_go_round() {
        assert_eq!(stepped(0, -1, 4), 3);
        assert_eq!(stepped(3, 1, 4), 0);
    }
}
