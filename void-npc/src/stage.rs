//! Where the view stands during a scene. Everything follows from where the NPC and the
//! player's double stand, so a shot works wherever a player starts a conversation.

use voidmc::DisplayTransform;
use voidmc::components::Position;

use crate::props::{self, Placing};

/// How far from the NPC the double stands: closer or further, a shot would not frame them.
pub const TALKING_DISTANCE: f64 = 2.6;
/// How long the view takes to go from one shot to the next, in ticks.
pub const GLIDE_TICKS: u32 = 14;
const EYES: f64 = 1.62;
const CHEST: f64 = 1.22;

/// A way of framing a scene.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Shot {
    /// Over the player's shoulder, the NPC ahead, as a conversation goes.
    #[default]
    Shoulder,
    /// The NPC on the left of the screen, which leaves the right to an interface.
    Portrait,
    /// The NPC's face at the top of the screen, which leaves the rest to an interface.
    Face,
    /// Offsets from the double, along the way to the NPC, to its right and up, then how
    /// far past the NPC the view aims, to the right of the way there.
    Custom {
        back: f64,
        right: f64,
        up: f64,
        aim_right: f64,
    },
}

/// Where a view stands and looks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
}

impl Pose {
    pub fn towards(from: [f64; 3], to: [f64; 3]) -> Self {
        let [x, y, z] = from;
        let (dx, dy, dz) = (to[0] - x, to[1] - y, to[2] - z);
        let flat = dx.hypot(dz);
        Self {
            x,
            y,
            z,
            yaw: (-dx).atan2(dz).to_degrees() as f32,
            pitch: (-dy).atan2(flat).to_degrees() as f32,
        }
    }

    /// `share` of the way to `other`, turning the short way round.
    pub fn blend(self, other: Pose, share: f64) -> Self {
        let mix = |from: f64, to: f64| from + (to - from) * share;
        let turn = (other.yaw - self.yaw + 180.0).rem_euclid(360.0) - 180.0;
        Self {
            x: mix(self.x, other.x),
            y: mix(self.y, other.y),
            z: mix(self.z, other.z),
            yaw: self.yaw + turn * share as f32,
            pitch: mix(f64::from(self.pitch), f64::from(other.pitch)) as f32,
        }
    }
}

/// Slow at both ends.
pub fn ease(share: f64) -> f64 {
    let share = share.clamp(0.0, 1.0);
    share * share * (3.0 - 2.0 * share)
}

/// The two who talk, and the way between them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stage {
    pub npc: Position,
    pub double: Position,
    /// From the double to the NPC, flat and of length one.
    forward: [f64; 2],
}

impl Stage {
    /// The double stands on the line from the NPC to where the player is.
    pub fn new(npc: Position, player: Position) -> Self {
        let (dx, dz) = (npc.x - player.x, npc.z - player.z);
        let length = dx.hypot(dz);
        let forward = if length < 0.01 {
            [0.0, 1.0]
        } else {
            [dx / length, dz / length]
        };
        let double = Position {
            x: npc.x - forward[0] * TALKING_DISTANCE,
            y: npc.y,
            z: npc.z - forward[1] * TALKING_DISTANCE,
        };
        Self {
            npc,
            double,
            forward,
        }
    }

    /// To the right of someone looking from the double to the NPC.
    fn right(&self) -> [f64; 2] {
        [-self.forward[1], self.forward[0]]
    }

    fn place(&self, from: Position, along: f64, right: f64, up: f64) -> [f64; 3] {
        let [fx, fz] = self.forward;
        let [rx, rz] = self.right();
        [
            from.x + fx * along + rx * right,
            from.y + up,
            from.z + fz * along + rz * right,
        ]
    }

    /// The way the double faces, and the way the NPC faces.
    pub fn facing(&self) -> (f32, f32) {
        let [fx, fz] = self.forward;
        let yaw = (-fx).atan2(fz).to_degrees() as f32;
        (yaw, yaw + 180.0)
    }

    /// The transformation of a prop on an entity at the feet of the NPC.
    pub(crate) fn placed(&self, placing: Placing) -> DisplayTransform {
        let [fx, fz] = self.forward;
        let near = [-fx, -fz];
        let facing = near[0].atan2(near[1]).to_degrees();
        props::transform(placing, self.right(), near, facing)
    }

    /// Where `placing` is in the world.
    pub(crate) fn spot(&self, placing: Placing) -> [f64; 3] {
        self.place(self.npc, -placing.near, placing.right, placing.up)
    }

    pub fn pose(&self, shot: Shot) -> Pose {
        match shot {
            Shot::Shoulder => Pose::towards(
                self.place(self.double, -1.25, 1.0, EYES + 0.04),
                self.place(self.npc, 0.0, 0.0, CHEST + 0.28),
            ),
            Shot::Portrait => Pose::towards(
                self.place(self.npc, -2.25, 0.6, EYES - 0.12),
                self.place(self.npc, 0.0, 1.3, CHEST + 0.18),
            ),
            Shot::Face => Pose::towards(
                self.place(self.npc, -1.5, 0.15, EYES),
                self.place(self.npc, 0.0, 0.0, EYES - 0.45),
            ),
            Shot::Custom {
                back,
                right,
                up,
                aim_right,
            } => Pose::towards(
                self.place(self.double, -back, right, up),
                self.place(self.npc, 0.0, aim_right, CHEST),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage() -> Stage {
        Stage::new(
            Position {
                x: 0.5,
                y: 65.0,
                z: 8.5,
            },
            Position {
                x: 0.5,
                y: 70.0,
                z: 2.0,
            },
        )
    }

    #[test]
    fn the_double_stands_at_talking_distance_on_the_ground_of_the_npc() {
        let stage = stage();
        assert_eq!(stage.double.y, 65.0);
        let apart = (stage.npc.z - stage.double.z).abs();
        assert!((apart - TALKING_DISTANCE).abs() < 1e-9);
    }

    #[test]
    fn the_two_face_each_other() {
        let (double, npc) = stage().facing();
        assert!(double.abs() < 1e-3);
        assert!((npc - 180.0).abs() < 1e-3);
    }

    #[test]
    fn a_player_on_top_of_the_npc_still_gets_a_way() {
        let spot = Position {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        };
        let stage = Stage::new(spot, spot);
        assert!(stage.pose(Shot::Shoulder).x.is_finite());
    }

    #[test]
    fn the_shoulder_view_stands_behind_and_to_the_right_of_the_double() {
        let stage = stage();
        let pose = stage.pose(Shot::Shoulder);
        assert!(pose.z < stage.double.z);
        assert!(pose.x < stage.double.x, "looking along +z, right is -x");
    }

    #[test]
    fn a_pose_looks_where_it_is_told() {
        let pose = Pose::towards([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        assert_eq!((pose.yaw, pose.pitch), (0.0, 0.0));
        let pose = Pose::towards([0.0, 0.0, 0.0], [-1.0, 0.0, 0.0]);
        assert!((pose.yaw - 90.0).abs() < 1e-3);
        let pose = Pose::towards([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
        assert!(pose.pitch > 0.0, "down is positive");
    }

    #[test]
    fn a_turn_goes_the_short_way_round() {
        let from = Pose::towards([0.0; 3], [0.0, 0.0, 1.0]);
        let to = Pose { yaw: 350.0, ..from };
        let half = from.blend(to, 0.5);
        assert!((half.yaw + 5.0).abs() < 1e-3);
    }

    #[test]
    fn easing_starts_and_ends_still() {
        assert_eq!(ease(0.0), 0.0);
        assert_eq!(ease(1.0), 1.0);
        assert!(ease(0.1) < 0.1);
        assert!(ease(0.9) > 0.9);
    }
}
