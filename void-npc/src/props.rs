//! Things a scene shows in the world, for its player only. A prop stands still as an entity:
//! its whole movement is in its transformation, which the client glides through between two
//! keyframes at the speed of the screen. So a prop turns smoothly on a keyframe every few ticks.

use voidmc::{DisplayTransform, Particle};

use crate::rig::Gesture;

/// A model of the pack, from [`crate::art::ArtBuilder::card`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prop {
    pub(crate) model: String,
}

/// Where a prop goes. Distances are in blocks from the feet of the NPC, and angles in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placing {
    /// To the right of someone facing the NPC.
    pub right: f64,
    pub up: f64,
    /// Towards the player.
    pub near: f64,
    /// Around its height: 180 shows its other side.
    pub turn: f64,
    /// Around its width, the top coming towards the player.
    pub tilt: f64,
    /// Around the way it faces, clockwise.
    pub roll: f64,
    pub scale: f64,
}

impl Default for Placing {
    fn default() -> Self {
        Self {
            right: 0.0,
            up: 0.0,
            near: 0.0,
            turn: 0.0,
            tilt: 0.0,
            roll: 0.0,
            scale: 1.0,
        }
    }
}

impl Placing {
    /// `share` of the way to `other`. A turn keeps every lap between them.
    pub fn blend(self, other: Placing, share: f64) -> Self {
        let mix = |from: f64, to: f64| from + (to - from) * share;
        Self {
            right: mix(self.right, other.right),
            up: mix(self.up, other.up),
            near: mix(self.near, other.near),
            turn: mix(self.turn, other.turn),
            tilt: mix(self.tilt, other.tilt),
            roll: mix(self.roll, other.roll),
            scale: mix(self.scale, other.scale),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Order {
    Show {
        slot: u32,
        model: String,
        placing: Placing,
        ticks: u16,
    },
    Hide(u32),
    Gesture(Gesture),
    Burst {
        particle: Particle,
        spot: Placing,
        count: i32,
    },
}

type Quaternion = [f64; 4];

fn around(axis: [f64; 3], degrees: f64) -> Quaternion {
    let (sin, cos) = (degrees.to_radians() / 2.0).sin_cos();
    [axis[0] * sin, axis[1] * sin, axis[2] * sin, cos]
}

fn times(a: Quaternion, b: Quaternion) -> Quaternion {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// The transformation of a prop on an entity at the feet of the NPC. `right` and `near` are
/// flat and of length one, and `facing` is the yaw that turns its front to the player.
pub(crate) fn transform(
    placing: Placing,
    right: [f64; 2],
    near: [f64; 2],
    facing: f64,
) -> DisplayTransform {
    let x = right[0] * placing.right + near[0] * placing.near;
    let z = right[1] * placing.right + near[1] * placing.near;
    let rotation = [
        around([0.0, 1.0, 0.0], facing + placing.turn),
        around([1.0, 0.0, 0.0], placing.tilt),
        around([0.0, 0.0, 1.0], -placing.roll),
    ]
    .into_iter()
    .fold([0.0, 0.0, 0.0, 1.0], times);
    let scale = placing.scale as f32;
    DisplayTransform::default()
        .translation(x as f32, placing.up as f32, z as f32)
        .uniform_scale(scale)
        .left_rotation(rotation.map(|part| part as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 4], b: [f32; 4]) -> bool {
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5)
    }

    #[test]
    fn a_prop_goes_where_the_stage_says() {
        let placing = Placing {
            right: 1.0,
            up: 2.0,
            near: 3.0,
            ..Placing::default()
        };
        let transform = transform(placing, [1.0, 0.0], [0.0, 1.0], 0.0);
        assert_eq!(transform.translation, [1.0, 2.0, 3.0]);
    }

    #[test]
    fn no_angle_is_no_rotation() {
        let transform = transform(Placing::default(), [1.0, 0.0], [0.0, 1.0], 0.0);
        assert!(close(transform.left_rotation, [0.0, 0.0, 0.0, 1.0]));
    }

    #[test]
    fn half_a_turn_turns_around_the_height() {
        let placing = Placing {
            turn: 180.0,
            ..Placing::default()
        };
        let transform = transform(placing, [1.0, 0.0], [0.0, 1.0], 0.0);
        assert!(close(transform.left_rotation, [0.0, 1.0, 0.0, 0.0]));
    }

    #[test]
    fn a_blend_keeps_the_laps() {
        let spun = Placing {
            turn: 720.0,
            ..Placing::default()
        };
        assert_eq!(Placing::default().blend(spun, 0.5).turn, 360.0);
    }
}
