//! The body of an NPC in a scene. A mannequin only stands there, so for the player who talks to
//! it the NPC is a figure of four item displays cut from its skin, whose head and arms turn on
//! their joints like any prop. Everyone else keeps seeing the mannequin.

use std::f64::consts::TAU;

use serde_json::{Value, json};

use crate::font::NAMESPACE;
use crate::props::Placing;
use crate::stage::ease;

/// The size of a player, drawn in pixels of a sixteenth of a block.
const SCALE: f64 = 0.9375;
const HIPS: f64 = 12.0;
const NECK: f64 = 24.0;
const SHOULDER: f64 = 22.0;
const WIDE_ARM: f64 = 4.0;
const SLIM_ARM: f64 = 3.0;
const OVERLAY: f64 = 0.4;

/// What an NPC does with its body, once, as a line starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    /// A raised hand, waving.
    Wave,
    /// A hand held out towards the player.
    Present,
    /// Both hands out, and the head to one side.
    Shrug,
    /// Two nods.
    Nod,
    /// A hand to the chin.
    Think,
}

impl Gesture {
    fn ticks(self) -> u32 {
        match self {
            Gesture::Wave => 46,
            Gesture::Present => 34,
            Gesture::Shrug => 34,
            Gesture::Nod => 26,
            Gesture::Think => 56,
        }
    }
}

/// The parts of a figure that move.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Joint {
    turn: f64,
    tilt: f64,
    roll: f64,
}

impl Joint {
    fn add(self, other: Joint, share: f64) -> Joint {
        Joint {
            turn: self.turn + other.turn * share,
            tilt: self.tilt + other.tilt * share,
            roll: self.roll + other.roll * share,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Limbs {
    pub head: Joint,
    pub right_arm: Joint,
    pub left_arm: Joint,
}

impl Limbs {
    fn add(self, other: Limbs, share: f64) -> Limbs {
        Limbs {
            head: self.head.add(other.head, share),
            right_arm: self.right_arm.add(other.right_arm, share),
            left_arm: self.left_arm.add(other.left_arm, share),
        }
    }
}

fn arm(tilt: f64, roll: f64) -> Joint {
    Joint {
        turn: 0.0,
        tilt,
        roll,
    }
}

fn wave(time: f64) -> f64 {
    (time * TAU).sin()
}

/// A body at rest, breathing. `time` is in ticks.
fn idle(time: f64) -> Limbs {
    let breath = wave(time / 70.0);
    Limbs {
        head: Joint {
            turn: wave(time / 130.0) * 3.0,
            tilt: breath * 1.2,
            roll: 0.0,
        },
        right_arm: arm(breath * 1.5, 2.5 + breath),
        left_arm: arm(-breath * 1.5, -2.5 - breath),
    }
}

/// A body that speaks: the hands go with the words.
fn talk(time: f64) -> Limbs {
    Limbs {
        head: Joint {
            turn: wave(time / 37.0) * 5.0,
            tilt: wave(time / 19.0) * 3.5,
            roll: wave(time / 53.0) * 2.0,
        },
        right_arm: arm(
            -34.0 + wave(time / 17.0) * 16.0,
            10.0 + wave(time / 29.0) * 6.0,
        ),
        left_arm: arm(-16.0 + wave(time / 23.0 + 0.3) * 10.0, -8.0),
    }
}

fn gesturing(gesture: Gesture, time: f64) -> Limbs {
    match gesture {
        Gesture::Wave => Limbs {
            head: Joint {
                roll: 6.0,
                ..Joint::default()
            },
            right_arm: arm(0.0, 148.0 + wave(time / 11.0) * 16.0),
            ..Limbs::default()
        },
        Gesture::Present => Limbs {
            head: Joint {
                tilt: 6.0,
                ..Joint::default()
            },
            right_arm: arm(-72.0, 22.0),
            ..Limbs::default()
        },
        Gesture::Shrug => Limbs {
            head: Joint {
                roll: 11.0,
                ..Joint::default()
            },
            right_arm: arm(-38.0, 34.0),
            left_arm: arm(-38.0, -34.0),
        },
        Gesture::Nod => Limbs {
            head: Joint {
                tilt: 8.0 + wave(time / 13.0) * 11.0,
                ..Joint::default()
            },
            ..Limbs::default()
        },
        Gesture::Think => Limbs {
            head: Joint {
                tilt: -7.0,
                roll: 7.0,
                ..Joint::default()
            },
            right_arm: arm(-118.0, -24.0),
            left_arm: arm(-24.0, 8.0),
        },
    }
}

/// How far into a gesture the body is: it goes there, stays, and comes back.
fn envelope(share: f64) -> f64 {
    const EDGE: f64 = 0.22;
    ease(share / EDGE).min(ease((1.0 - share) / EDGE))
}

/// What a figure is doing, from one keyframe to the next.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Motion {
    ticks: u32,
    gesture: Option<(Gesture, u32)>,
    speech: f64,
}

impl Motion {
    pub fn play(&mut self, gesture: Gesture) {
        self.gesture = Some((gesture, 0));
    }

    /// `step` ticks later, for a body that is speaking or not.
    pub fn advance(&mut self, step: u32, speaking: bool) -> Limbs {
        self.ticks += step;
        let target = if speaking { 1.0 } else { 0.0 };
        let change = f64::from(step) / 8.0;
        self.speech += (target - self.speech).clamp(-change, change);
        let time = f64::from(self.ticks);
        let mut limbs = idle(time).add(talk(time), ease(self.speech));
        if let Some((gesture, begun)) = &mut self.gesture {
            *begun += step;
            let share = f64::from(*begun) / f64::from(gesture.ticks());
            if share >= 1.0 {
                self.gesture = None;
            } else {
                let weight = envelope(share);
                let shown = gesturing(*gesture, f64::from(*begun));
                limbs = limbs.add(limbs, -weight).add(shown, weight);
            }
        }
        limbs
    }
}

fn placed(right: f64, up: f64, joint: Joint) -> Placing {
    Placing {
        right: right / 16.0 * SCALE,
        up: up / 16.0 * SCALE,
        near: 0.0,
        turn: joint.turn,
        tilt: joint.tilt,
        roll: joint.roll,
        scale: SCALE,
    }
}

/// The parts of a figure, in the order of [`Figure::parts`].
pub(crate) const PARTS: [&str; 4] = ["body", "head", "right_arm", "left_arm"];

pub(crate) fn placings(limbs: Limbs, slim: bool) -> [Placing; 4] {
    let shoulder = 4.0 + arm_width(slim) / 2.0;
    [
        placed(0.0, HIPS, Joint::default()),
        placed(0.0, NECK, limbs.head),
        placed(-shoulder, SHOULDER, limbs.right_arm),
        placed(shoulder, SHOULDER, limbs.left_arm),
    ]
}

fn arm_width(slim: bool) -> f64 {
    if slim { SLIM_ARM } else { WIDE_ARM }
}

/// A box of a skin: where it stands in the model, and where its faces start on the picture.
struct Cuboid {
    from: [f64; 3],
    size: [f64; 3],
    skin: [f64; 2],
}

fn region(x: f64, y: f64, width: f64, height: f64) -> Value {
    json!([x / 4.0, y / 4.0, (x + width) / 4.0, (y + height) / 4.0])
}

impl Cuboid {
    /// A model's north is the front of the figure, so its east is the figure's right.
    fn element(&self, grow: f64, skin: [f64; 2]) -> Value {
        let [width, height, depth] = self.size;
        let [u, v] = skin;
        let [x, y, z] = self.from;
        let face = |uv: Value| json!({ "uv": uv, "texture": "#skin" });
        let top = json!({
            "uv": region(u + depth, v, width, depth),
            "texture": "#skin",
            "rotation": 180,
        });
        json!({
            "from": [x - grow, y - grow, z - grow],
            "to": [x + width + grow, y + height + grow, z + depth + grow],
            "faces": {
                "east": face(region(u, v + depth, depth, height)),
                "north": face(region(u + depth, v + depth, width, height)),
                "west": face(region(u + depth + width, v + depth, depth, height)),
                "south": face(region(u + depth * 2.0 + width, v + depth, width, height)),
                "up": top,
                "down": face(region(u + depth + width, v, width, depth)),
            },
        })
    }
}

/// Everything of a part, around the joint at the middle of the item: the box and the layer
/// over it, whose place on a skin is `overlay`.
fn with_overlay(cuboid: Cuboid, overlay: [f64; 2]) -> [Value; 2] {
    [
        cuboid.element(0.0, cuboid.skin),
        cuboid.element(OVERLAY, overlay),
    ]
}

/// The elements of the model of part `index` of [`PARTS`].
fn elements(index: usize, slim: bool) -> Vec<Value> {
    let arm = arm_width(slim);
    let hanging = |skin: [f64; 2]| Cuboid {
        from: [8.0 - arm / 2.0, -2.0, 6.0],
        size: [arm, 12.0, 4.0],
        skin,
    };
    match index {
        0 => [
            with_overlay(
                Cuboid {
                    from: [4.0, 8.0, 6.0],
                    size: [8.0, 12.0, 4.0],
                    skin: [16.0, 16.0],
                },
                [16.0, 32.0],
            ),
            with_overlay(
                Cuboid {
                    from: [8.0, -4.0, 6.0],
                    size: [4.0, 12.0, 4.0],
                    skin: [0.0, 16.0],
                },
                [0.0, 32.0],
            ),
            with_overlay(
                Cuboid {
                    from: [4.0, -4.0, 6.0],
                    size: [4.0, 12.0, 4.0],
                    skin: [16.0, 48.0],
                },
                [0.0, 48.0],
            ),
        ]
        .concat(),
        1 => with_overlay(
            Cuboid {
                from: [4.0, 8.0, 4.0],
                size: [8.0, 8.0, 8.0],
                skin: [0.0, 0.0],
            },
            [32.0, 0.0],
        )
        .to_vec(),
        2 => with_overlay(hanging([40.0, 16.0]), [40.0, 32.0]).to_vec(),
        _ => with_overlay(hanging([32.0, 48.0]), [48.0, 48.0]).to_vec(),
    }
}

/// The item models of a figure, from a skin in the pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Figure {
    pub parts: [String; 4],
    pub slim: bool,
}

impl Figure {
    pub fn new(npc: &str, slim: bool) -> Self {
        Self {
            parts: PARTS.map(|part| format!("{NAMESPACE}:{}", file_name(npc, part))),
            slim,
        }
    }
}

fn file_name(npc: &str, part: &str) -> String {
    format!("figure_{npc}_{part}")
}

/// The files of the models of a figure, as paths and contents. The skin goes at
/// [`texture_file`].
pub(crate) fn files(npc: &str, slim: bool) -> Vec<(String, Vec<u8>)> {
    let texture = format!("{NAMESPACE}:item/figure_{npc}");
    PARTS
        .iter()
        .enumerate()
        .flat_map(|(index, part)| {
            let name = file_name(npc, part);
            let model = json!({
                "textures": { "particle": texture, "skin": texture },
                "elements": elements(index, slim),
            });
            let item = json!({
                "model": { "type": "minecraft:model", "model": format!("{NAMESPACE}:item/{name}") },
            });
            [
                (
                    format!("assets/{NAMESPACE}/models/item/{name}.json"),
                    model.to_string().into_bytes(),
                ),
                (
                    format!("assets/{NAMESPACE}/items/{name}.json"),
                    item.to_string().into_bytes(),
                ),
            ]
        })
        .collect()
}

pub(crate) fn texture_file(npc: &str) -> String {
    format!("assets/{NAMESPACE}/textures/item/figure_{npc}.png")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gesture_ends_and_the_body_goes_back_to_rest() {
        let mut motion = Motion::default();
        motion.play(Gesture::Nod);
        for _ in 0..Gesture::Nod.ticks() {
            motion.advance(1, false);
        }
        assert_eq!(motion.gesture, None);
    }

    #[test]
    fn a_wave_raises_the_hand_past_the_head() {
        let mut motion = Motion::default();
        motion.play(Gesture::Wave);
        let limbs = (0..Gesture::Wave.ticks() / 2)
            .map(|_| motion.advance(1, false))
            .last()
            .unwrap();
        assert!(limbs.right_arm.roll > 120.0);
    }

    #[test]
    fn a_body_starts_and_stops_speaking_gently() {
        let mut motion = Motion::default();
        let first = motion.advance(2, true);
        assert!(first.right_arm.tilt > -5.0);
        for _ in 0..8 {
            motion.advance(2, true);
        }
        assert_eq!(motion.speech, 1.0);
    }

    #[test]
    fn no_part_leaves_what_a_model_can_hold() {
        for slim in [false, true] {
            for index in 0..PARTS.len() {
                for element in elements(index, slim) {
                    for corner in ["from", "to"] {
                        for at in element[corner].as_array().unwrap() {
                            let at = at.as_f64().unwrap();
                            assert!((-16.0..=32.0).contains(&at));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_right_arm_stands_on_the_left_of_who_faces_the_figure() {
        let [_, _, right, left] = placings(Limbs::default(), false);
        assert!(right.right < 0.0 && left.right > 0.0);
    }
}
