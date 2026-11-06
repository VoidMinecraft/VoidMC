//! The way between the world and the map as a small film: a saucer takes the player away,
//! and brings them back. This says what happens at each tick of a [`Scene`], with no
//! entities, and `plugin::abduction` plays it. The clock is the flight of the camera, so
//! that the film waits with it and stops with it.

use bevy_ecs::prelude::Component;
use voidmc_ui::{Image, ResourcePack};

use crate::pointer::{SPECTATOR_EYE_HEIGHT, ease, look_direction};

/// How long each film lasts, with the camera in the eye at one end and over the land at
/// the other.
pub const INTRO_TICKS: u32 = 104;
pub const OUTRO_TICKS: u32 = 104;

/// How far over the feet the saucer hangs while the beam is on.
pub const HOVER: f64 = 8.5;
/// How much of the height of the saucer lies under where it is placed.
const HULL_BELOW: f64 = 0.65;
/// Straight down, as a look.
const DOWN: f32 = 90.0;
/// How far the camera goes up on its way to the map.
const SKY_HEIGHT: f64 = 34.0;
/// Where the camera watches from: behind the player, off to a side, over their head.
const WATCH_BACK: f64 = 9.5;
const WATCH_SIDE: f64 = 5.0;
const WATCH_HEIGHT: f64 = 1.4;
/// The same, close enough to see a body struggle.
const CLOSE_BACK: f64 = 5.5;
const CLOSE_SIDE: f64 = 3.0;
/// The way back is seen from far and high, with the saucer, the beam and the body in the land
/// around them. The camera goes slowly round from here to there as it all happens.
const WIDE_BACK: f64 = 15.0;
const WIDE_SIDE: f64 = 8.5;
const WIDE_HEIGHT: f64 = 9.0;
const TURNED_BACK: f64 = 13.0;
const TURNED_SIDE: f64 = 4.0;
const TURNED_HEIGHT: f64 = 7.0;
/// What the wide view is aimed at: between the saucer and the feet.
const WIDE_AIM: f64 = 4.2;
/// Halfway down from there to the head.
const NEARING_BACK: f64 = 6.5;
const NEARING_SIDE: f64 = 2.0;
const NEARING_HEIGHT: f64 = 3.4;
/// The way out of an eye, and back into it: straight up through the top of the head and
/// behind it from there. Coming along the look, the back of the head would fill the screen.
const OVER_THE_HEAD: f64 = 0.85;
const LEVEL_PITCH: f32 = 12.0;
/// How far around the beam a body swings as it goes, and how fast. A client swings the
/// legs from how far an entity goes in a tick, and is at full swing from a quarter block.
const SWING_RADIUS: f64 = 0.75;
const SWING_PER_TICK: f64 = 0.62;
/// The same on the ground, where it passes for running.
const RUN_RADIUS: f64 = 0.4;

/// The moments of the intro.
mod intro {
    pub const SAUCER_IN: u32 = 8;
    pub const SAUCER_OVER: u32 = 30;
    pub const LOOKS_UP: u32 = 14;
    pub const PANIC: u32 = 24;
    pub const BEAM_ON: u32 = 32;
    pub const LIFT: u32 = 38;
    pub const WHIRL: u32 = 66;
    pub const TAKEN: u32 = 76;
    pub const BEAM_OFF: u32 = 78;
    pub const DEPART: u32 = 80;
    pub const GONE: u32 = 98;
    pub const WATCHING: u32 = 28;
    pub const CLOSE: u32 = 46;
    pub const FOLLOWED: u32 = 78;
    /// The body stands in the way of the eye until the camera is out of the head.
    pub const BODY: u32 = 11;
}

/// The moments of the outro.
mod outro {
    pub const SAUCER_OVER: u32 = 26;
    pub const BEAM_ON: u32 = 28;
    pub const OUT: u32 = 34;
    pub const LANDED: u32 = 70;
    pub const STANDS: u32 = 74;
    pub const CALM: u32 = 88;
    pub const BEAM_OFF: u32 = 72;
    pub const SPARKS_END: u32 = 46;
    pub const DEPART: u32 = 74;
    pub const GONE: u32 = 94;
    pub const WIDE: u32 = 32;
    pub const TURNED: u32 = 72;
    pub const NEARING: u32 = 86;
    pub const OVER_THE_HEAD: u32 = 96;
    /// A client draws the inside of a head: the body goes before the camera gets there.
    pub const BODY_GONE: u32 = 98;
}

/// Taken, or brought back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Intro,
    Outro,
}

/// A point of a way, with the look at that point.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Key {
    tick: u32,
    at: [f64; 3],
    look: [f32; 2],
}

impl Key {
    const fn new(tick: u32, at: [f64; 3], look: [f32; 2]) -> Self {
        Self { tick, at, look }
    }

    /// Its numbers side by side, for a way to treat them all alike.
    fn values(&self) -> [f64; 5] {
        let [x, y, z] = self.at;
        let [yaw, pitch] = self.look;
        [x, y, z, f64::from(yaw), f64::from(pitch)]
    }
}

/// How fast a number goes through each key: a way only stops at both ends and where it turns
/// back, and goes through the others without a change of speed. It never overshoots a key,
/// so that what holds still between two keys does hold still.
fn slopes(keys: &[Key], value: usize) -> Vec<f64> {
    let span = |index: usize| f64::from(keys[index + 1].tick - keys[index].tick);
    let rise = |index: usize| keys[index + 1].values()[value] - keys[index].values()[value];
    let mut slopes = vec![0.0; keys.len()];
    for (index, slope) in slopes.iter_mut().enumerate().take(keys.len() - 1).skip(1) {
        let (before, after) = (span(index - 1), span(index));
        let (first, second) = (rise(index - 1) / before, rise(index) / after);
        if first * second > 0.0 {
            let (near, far) = (2.0 * after + before, after + 2.0 * before);
            *slope = (near + far) / (near / first + far / second);
        }
    }
    slopes
}

/// The way through its keys at some tick, in one curve.
fn along(keys: &[Key], tick: f64) -> ([f64; 3], [f32; 2]) {
    let (first, last) = (keys[0], keys[keys.len() - 1]);
    if tick <= f64::from(first.tick) {
        return (first.at, first.look);
    }
    let Some(index) = keys
        .windows(2)
        .position(|pair| tick < f64::from(pair[1].tick))
    else {
        return (last.at, last.look);
    };
    let (from, to) = (keys[index], keys[index + 1]);
    let span = f64::from(to.tick - from.tick);
    let share = (tick - f64::from(from.tick)) / span;
    let (square, cube) = (share * share, share * share * share);
    let value = |axis: usize| {
        let slope = slopes(keys, axis);
        (2.0 * cube - 3.0 * square + 1.0) * from.values()[axis]
            + (cube - 2.0 * square + share) * span * slope[index]
            + (3.0 * square - 2.0 * cube) * to.values()[axis]
            + (cube - square) * span * slope[index + 1]
    };
    (
        std::array::from_fn(value),
        std::array::from_fn(|axis| value(axis + 3) as f32),
    )
}

/// What a body does, which a client then animates by itself.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Stance {
    #[default]
    Stand,
    /// Trembling on its feet.
    Shiver,
    /// Crouched.
    Cower,
    /// Upright, shaking, with the limbs going.
    Flail,
    /// Tipped over: its pitch turns the whole body.
    Tumble,
    /// Spinning around its own length.
    Whirl,
}

/// The body at some tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Body {
    pub at: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub stance: Stance,
}

/// Something to hear or see once, at some tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    SaucerComes,
    BeamOn,
    Grabbed,
    Taken,
    SaucerGoes,
    Dropped,
    Landed,
}

/// Which way around the player the film is shot, for the land to leave room for the camera.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct View {
    /// Degrees from the way the player looks.
    pub turn: f32,
    /// How far over its usual height the camera goes.
    pub lift: f64,
}

/// One film, from where the player's eye is and how they look. All of it follows from that.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scene {
    part: Part,
    eye: [f64; 3],
    look: [f32; 2],
    view: View,
}

impl Scene {
    pub fn new(part: Part, eye: [f64; 3], look: [f32; 2]) -> Self {
        Self {
            part,
            eye,
            look,
            view: View::default(),
        }
    }

    /// The same film from another side. The camera only gets there after a few ticks.
    pub fn seen_from(self, view: View) -> Self {
        Self { view, ..self }
    }

    /// The sides to try, the one behind the player first.
    pub fn turns() -> [f32; 4] {
        [0.0, 90.0, -90.0, 180.0]
    }

    /// Where a camera shot from this side goes, for the land to be checked.
    pub fn stops(&self, turn: f32) -> Vec<[f64; 3]> {
        let around = self.seen_from(View { turn, lift: 0.0 });
        match self.part {
            Part::Intro => {
                let [x, y, z] = around.off_feet(WATCH_BACK, WATCH_SIDE, WATCH_HEIGHT);
                let [feet_x, _, feet_z] = self.feet();
                vec![
                    [x, y, z],
                    [(x + feet_x) / 2.0, y, (z + feet_z) / 2.0],
                    around.off_feet(CLOSE_BACK, CLOSE_SIDE, 0.9),
                    around.off_feet(CLOSE_BACK / 2.0, CLOSE_SIDE / 2.0, 0.9),
                ]
            }
            Part::Outro => vec![
                around.off_feet(WIDE_BACK, WIDE_SIDE, WIDE_HEIGHT),
                around.off_feet(TURNED_BACK, TURNED_SIDE, TURNED_HEIGHT),
                around.off_feet(NEARING_BACK, NEARING_SIDE, NEARING_HEIGHT),
                around.off_feet(NEARING_BACK / 2.0, NEARING_SIDE / 2.0, NEARING_HEIGHT / 2.0),
            ],
        }
    }

    pub fn part(&self) -> Part {
        self.part
    }

    pub fn length(&self) -> u32 {
        match self.part {
            Part::Intro => INTRO_TICKS,
            Part::Outro => OUTRO_TICKS,
        }
    }

    /// Where the player stands.
    pub fn feet(&self) -> [f64; 3] {
        let [x, y, z] = self.eye;
        [x, y - SPECTATOR_EYE_HEIGHT, z]
    }

    /// `back` blocks behind the look, `side` to its right, `up` over the feet.
    fn off_feet(&self, back: f64, side: f64, up: f64) -> [f64; 3] {
        let [x, y, z] = self.feet();
        let way = look_direction(self.look[0] + self.view.turn, 0.0);
        [
            x - way[0] * back - way[2] * side,
            y + up,
            z - way[2] * back + way[0] * side,
        ]
    }

    /// The look from one place to another, with its yaw on the same turn as the player's.
    fn aim(&self, from: [f64; 3], to: [f64; 3]) -> [f32; 2] {
        let [x, y, z] = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
        let yaw = (-x).atan2(z).to_degrees() as f32;
        let pitch = (-y).atan2(x.hypot(z)).to_degrees() as f32;
        let off = (yaw - self.look[0] + 180.0).rem_euclid(360.0) - 180.0;
        [self.look[0] + off, pitch]
    }

    /// The camera, from the eye up to the sky, or from the sky down to the eye.
    fn camera(&self, tick: f64) -> ([f64; 3], [f32; 2]) {
        let eye = self.eye;
        let over_head = [eye[0], eye[1] + OVER_THE_HEAD, eye[2]];
        let feet = self.feet();
        let middle = |up| [feet[0], feet[1] + up, feet[2]];
        let lift = self.view.lift;
        let watch = self.off_feet(WATCH_BACK, WATCH_SIDE, WATCH_HEIGHT + lift);
        // Level, whatever the look: from over the head, a look down would have it in frame.
        let level = [self.look[0], self.look[1].min(LEVEL_PITCH)];
        let sky = [feet[0], feet[1] + SKY_HEIGHT, feet[2]];
        let sky_look = [self.look[0], DOWN];
        match self.part {
            Part::Intro => {
                use intro::{CLOSE, FOLLOWED, WATCHING};
                let close = self.off_feet(CLOSE_BACK, CLOSE_SIDE, 0.9 + lift);
                let followed = self.off_feet(CLOSE_BACK, CLOSE_SIDE, 2.6 + lift);
                let chase = self.off_feet(-12.0, 4.0, 30.0);
                let keys = [
                    Key::new(crate::transition::SWAP_TICKS, eye, self.look),
                    Key::new(12, over_head, level),
                    Key::new(WATCHING, watch, self.aim(watch, middle(4.4))),
                    Key::new(CLOSE, close, self.aim(close, middle(3.0))),
                    Key::new(FOLLOWED, followed, self.aim(followed, middle(HOVER))),
                    Key::new(INTRO_TICKS, chase, self.aim(chase, self.saucer_exit())),
                ];
                along(&keys, tick)
            }
            Part::Outro => {
                use outro::{NEARING, OVER_THE_HEAD as ABOVE, TURNED, WIDE};
                let wide = self.off_feet(WIDE_BACK, WIDE_SIDE, WIDE_HEIGHT + lift);
                let turned = self.off_feet(TURNED_BACK, TURNED_SIDE, TURNED_HEIGHT + lift);
                let nearing = self.off_feet(NEARING_BACK, NEARING_SIDE, NEARING_HEIGHT + lift);
                let keys = [
                    Key::new(0, sky, sky_look),
                    Key::new(WIDE, wide, self.aim(wide, middle(WIDE_AIM))),
                    Key::new(TURNED, turned, self.aim(turned, middle(WIDE_AIM))),
                    Key::new(NEARING, nearing, self.aim(nearing, middle(2.6))),
                    Key::new(ABOVE, over_head, level),
                    Key::new(OUTRO_TICKS, eye, self.look),
                ];
                along(&keys, tick)
            }
        }
    }

    /// Where the saucer goes off to: ahead of the player, into the sky.
    fn saucer_exit(&self) -> [f64; 3] {
        self.off_feet(-90.0, 24.0, 62.0)
    }

    /// The place of the saucer, and how it leans, as yaw and pitch.
    pub fn saucer(&self, tick: u32) -> ([f64; 3], [f32; 2]) {
        let over = self.off_feet(0.0, 0.0, HOVER);
        let far = self.off_feet(-44.0, 10.0, 30.0);
        let gone = self.saucer_exit();
        let high = self.off_feet(-6.0, -20.0, 60.0);
        let flat = [0.0, 0.0];
        let keys: &[Key] = match self.part {
            Part::Intro => &[
                Key::new(intro::SAUCER_IN, far, [0.0, 14.0]),
                Key::new(intro::SAUCER_OVER, over, flat),
                Key::new(intro::DEPART, over, flat),
                Key::new(intro::GONE, gone, [0.0, -18.0]),
            ],
            Part::Outro => &[
                Key::new(0, high, [0.0, 14.0]),
                Key::new(outro::SAUCER_OVER, over, flat),
                Key::new(outro::DEPART, over, flat),
                Key::new(outro::GONE, gone, [0.0, -18.0]),
            ],
        };
        let (mut at, [_, pitch]) = along(keys, f64::from(tick));
        let bob = (f64::from(tick) * 0.22).sin() * 0.18;
        at[1] += bob;
        (at, [tick as f32 * SAUCER_SPIN, pitch])
    }

    /// Whether the saucer has anything to show yet, and still has.
    pub fn saucer_shown(&self, tick: u32) -> bool {
        match self.part {
            Part::Intro => tick >= intro::SAUCER_IN.saturating_sub(4),
            Part::Outro => true,
        }
    }

    /// The saucer is out of sight.
    pub fn saucer_gone(&self, tick: u32) -> bool {
        match self.part {
            Part::Intro => tick >= intro::GONE,
            Part::Outro => tick >= outro::GONE,
        }
    }

    /// Whether the beam is on.
    pub fn beam(&self, tick: u32) -> bool {
        match self.part {
            Part::Intro => (intro::BEAM_ON..intro::BEAM_OFF).contains(&tick),
            Part::Outro => (outro::BEAM_ON..outro::BEAM_OFF).contains(&tick),
        }
    }

    /// Whether the beam gives off sparks. They hang for seconds, and on the way back the
    /// camera goes through where they are.
    pub fn sparks(&self, tick: u32) -> bool {
        self.beam(tick) && (self.part == Part::Intro || tick < outro::SPARKS_END)
    }

    /// The body at some tick, if it is to be seen.
    pub fn body(&self, tick: u32) -> Option<Body> {
        match self.part {
            Part::Intro => self.taken(tick),
            Part::Outro => self.brought_back(tick),
        }
    }

    /// On a body swinging around the beam at some height, facing the way the swing goes.
    fn swinging(&self, tick: u32, height: f64, radius: f64, stance: Stance) -> Body {
        let angle = f64::from(tick) * SWING_PER_TICK;
        let [x, y, z] = self.feet();
        let flip = (f64::from(tick) * 0.4).sin() as f32;
        Body {
            at: [
                x + angle.cos() * radius,
                y + height,
                z + angle.sin() * radius,
            ],
            yaw: (angle.to_degrees() as f32) + 180.0,
            pitch: match stance {
                Stance::Tumble => flip * 80.0,
                Stance::Whirl => -90.0,
                _ => flip * 50.0 - 20.0,
            },
            stance,
        }
    }

    fn taken(&self, tick: u32) -> Option<Body> {
        use intro::{BODY, LIFT, LOOKS_UP, PANIC, TAKEN, WHIRL};
        let [yaw, pitch] = self.look;
        let t = f64::from(tick);
        if !(BODY..TAKEN).contains(&tick) {
            return None;
        }
        if tick < PANIC {
            let up = ease(((t - f64::from(LOOKS_UP)) / 8.0).clamp(0.0, 1.0)) as f32;
            return Some(Body {
                at: self.feet(),
                yaw,
                pitch: pitch + (-65.0 - pitch) * up,
                stance: Stance::Stand,
            });
        }
        if tick < LIFT {
            // Running on the spot, for all the good it does.
            let radius = RUN_RADIUS * ease(((t - f64::from(PANIC)) / 5.0).min(1.0));
            return Some(self.swinging(tick, 0.0, radius, Stance::Shiver));
        }
        let share = (t - f64::from(LIFT)) / f64::from(TAKEN - LIFT);
        let height = ease(share) * (HOVER - HULL_BELOW + 0.4);
        let swell = (std::f64::consts::PI * share).sin();
        let radius = RUN_RADIUS + (SWING_RADIUS - RUN_RADIUS) * swell;
        let stance = match tick {
            _ if tick >= WHIRL => Stance::Whirl,
            _ if ((tick - LIFT) / 9).is_multiple_of(2) => Stance::Flail,
            _ => Stance::Tumble,
        };
        Some(self.swinging(tick, height, radius, stance))
    }

    fn brought_back(&self, tick: u32) -> Option<Body> {
        use outro::{BODY_GONE, CALM, LANDED, OUT, STANDS};
        let [yaw, pitch] = self.look;
        let at = self.feet();
        let t = f64::from(tick);
        if !(OUT..BODY_GONE).contains(&tick) {
            return None;
        }
        if tick < LANDED {
            let share = (t - f64::from(OUT)) / f64::from(LANDED - OUT);
            let height = (1.0 - ease(share)) * (HOVER - HULL_BELOW + 0.4);
            let radius = SWING_RADIUS * (std::f64::consts::PI * share).sin().sqrt();
            let stance = if ((tick - OUT) / 9).is_multiple_of(2) {
                Stance::Tumble
            } else {
                Stance::Flail
            };
            return Some(self.swinging(tick, height, radius, stance));
        }
        if tick < STANDS {
            return Some(Body {
                at,
                yaw,
                pitch: 40.0,
                stance: Stance::Cower,
            });
        }
        // Dizzy, then back to how the player looks, for the camera to take over from it.
        let calm = ease(((t - f64::from(STANDS)) / f64::from(CALM - STANDS)).min(1.0));
        let staggering = self.swinging(tick, 0.0, RUN_RADIUS * (1.0 - calm), Stance::Shiver);
        let calm = calm as f32;
        let dizzy = ((t * 0.55).sin() * 45.0) as f32 * (1.0 - calm);
        Some(Body {
            at: staggering.at,
            yaw: yaw + dizzy,
            pitch: pitch * calm + 12.0 * (1.0 - calm),
            stance: if calm < 1.0 {
                Stance::Shiver
            } else {
                Stance::Stand
            },
        })
    }

    /// What happens right at this tick.
    pub fn cue(&self, tick: u32) -> Option<Cue> {
        match self.part {
            Part::Intro => match tick {
                intro::SAUCER_IN => Some(Cue::SaucerComes),
                intro::BEAM_ON => Some(Cue::BeamOn),
                intro::LIFT => Some(Cue::Grabbed),
                intro::TAKEN => Some(Cue::Taken),
                intro::DEPART => Some(Cue::SaucerGoes),
                _ => None,
            },
            Part::Outro => match tick {
                1 => Some(Cue::SaucerComes),
                outro::BEAM_ON => Some(Cue::BeamOn),
                outro::OUT => Some(Cue::Dropped),
                outro::LANDED => Some(Cue::Landed),
                outro::DEPART => Some(Cue::SaucerGoes),
                _ => None,
            },
        }
    }
}

/// A way of the camera, for `Flight` to fly: it starts and ends where the scene says.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Track(pub Scene);

impl Track {
    pub fn at(&self, tick: f64) -> ([f64; 3], [f32; 2]) {
        self.0.camera(tick)
    }
}

/// Degrees the saucer turns in a tick.
const SAUCER_SPIN: f32 = 14.0;

/// What an item takes to look like each part of the scene, in its `item_model`.
pub const SAUCER_MODEL: &str = "voidmap:saucer";
pub const BEAM_MODEL: &str = "voidmap:saucer_beam";

/// A disc of the saucer: how far it reaches from the middle and between which heights, in
/// sixteenths of a block around the middle of the item, and the name of its texture.
const HULL: [(f32, f32, f32, &str); 8] = [
    (5.0, 5.0, 6.0, "underside"),
    (10.0, 6.0, 7.0, "hull"),
    (14.0, 7.0, 8.0, "hull"),
    (16.0, 8.0, 9.0, "rim"),
    (13.0, 9.0, 10.0, "hull"),
    (8.5, 10.0, 11.0, "hull"),
    (5.5, 11.0, 13.0, "dome"),
    (3.5, 13.0, 14.5, "dome"),
];
/// A disc is made of boxes whose corners lie on its circle, at these angles.
const CORNERS: [f32; 5] = [12.0, 27.0, 45.0, 63.0, 78.0];
const TEXTURE_SIZE: u32 = 16;
const BEAM_SIZE: (u32, u32) = (64, 256);

fn hull_model() -> String {
    let element = |&(reach, low, high, texture): &(f32, f32, f32, &str), corner: f32| {
        let (sin, cos) = corner.to_radians().sin_cos();
        let (x, z) = (reach * cos, reach * sin);
        // A box past the sides of a block would take its corners from the rest of the atlas.
        let face =
            |side: &str, uv: &str| format!(r##""{side}":{{"texture":"#{texture}","uv":{uv}}}"##);
        let faces: Vec<_> = ["north", "south", "east", "west"]
            .map(|side| face(side, "[0,6,16,10]"))
            .into_iter()
            .chain(["up", "down"].map(|side| face(side, "[0,0,16,16]")))
            .collect();
        format!(
            r#"{{"from":[{},{low},{}],"to":[{},{high},{}],"faces":{{{}}}}}"#,
            8.0 - x,
            8.0 - z,
            8.0 + x,
            8.0 + z,
            faces.join(",")
        )
    };
    let elements: Vec<_> = HULL
        .iter()
        .flat_map(|layer| CORNERS.map(|corner| element(layer, corner)))
        .collect();
    let texture = |name: &str| format!(r#""{name}":"voidmap:item/saucer_{name}""#);
    let textures = ["hull", "rim", "dome", "underside"].map(texture);
    format!(
        r#"{{"textures":{{"particle":"voidmap:item/saucer_hull",{}}},"elements":[{}]}}"#,
        textures.join(","),
        elements.join(",")
    )
}

/// A single sheet of light: the display turns it to the camera.
fn beam_model() -> String {
    let face = r##"{"texture":"#beam","uv":[0,0,16,16]}"##;
    format!(
        r#"{{"textures":{{"particle":"voidmap:item/saucer_beam","beam":"voidmap:item/saucer_beam"}},"elements":[{{"from":[0,0,8],"to":[16,16,8],"shade":false,"faces":{{"north":{face},"south":{face}}}}}]}}"#
    )
}

/// Painted from a formula of the place on the texture.
fn painted(width: u32, height: u32, paint: impl Fn(u32, u32) -> [u8; 4]) -> Image {
    let mut image = Image::new(width, height);
    for y in 0..height {
        for x in 0..width {
            image.set(x, y, paint(x, y));
        }
    }
    image
}

fn hull_texture() -> Image {
    painted(TEXTURE_SIZE, TEXTURE_SIZE, |x, y| {
        let shade = 150 + (((x * 7 + y * 13) % 5) * 6) as u8;
        [shade, shade + 8, shade + 22, 255]
    })
}

fn rim_texture() -> Image {
    painted(TEXTURE_SIZE, TEXTURE_SIZE, |x, y| {
        let light = x % 8 < 3 && (5..11).contains(&y);
        if light {
            [120, 255, 170, 255]
        } else {
            [58, 66, 84, 255]
        }
    })
}

fn dome_texture() -> Image {
    painted(TEXTURE_SIZE, TEXTURE_SIZE, |x, y| {
        let glint = (x + y) % 11 < 2;
        if glint {
            [220, 255, 245, 200]
        } else {
            [90, 230, 200, 130]
        }
    })
}

fn underside_texture() -> Image {
    painted(TEXTURE_SIZE, TEXTURE_SIZE, |_, _| [190, 255, 225, 255])
}

/// A cone of light, narrow at the top, soft at its edges and fading a little towards the
/// ground, with bands in it that give it some grain.
fn beam_texture() -> Image {
    let (width, height) = BEAM_SIZE;
    painted(width, height, |x, y| {
        let down = f64::from(y) / f64::from(height - 1);
        let across = (f64::from(x) + 0.5) / f64::from(width) * 2.0 - 1.0;
        let reach = 0.28 + 0.72 * down;
        let edge = (1.0 - (across.abs() / reach)).max(0.0);
        let glow = edge.powf(0.7);
        let core = (1.0 - across.abs() / (reach * 0.35)).max(0.0);
        let bands = 0.9 + 0.1 * (down * 40.0).sin();
        let alpha = (glow * 0.62 + core * 0.35) * bands * (1.0 - 0.3 * down);
        let white = core * 0.6;
        let tint = |base: f64| (base + (255.0 - base) * white) as u8;
        [
            tint(120.0),
            tint(255.0),
            tint(190.0),
            (alpha * 255.0).min(255.0) as u8,
        ]
    })
}

/// Adds the saucer and its beam.
pub fn install(pack: &mut ResourcePack) {
    let item = |name: &str| {
        format!(r#"{{"model":{{"type":"minecraft:model","model":"voidmap:item/{name}"}}}}"#)
    };
    pack.add(
        "assets/voidmap/items/saucer.json",
        item("saucer").into_bytes(),
    );
    pack.add(
        "assets/voidmap/items/saucer_beam.json",
        item("saucer_beam").into_bytes(),
    );
    pack.add(
        "assets/voidmap/models/item/saucer.json",
        hull_model().into_bytes(),
    );
    pack.add(
        "assets/voidmap/models/item/saucer_beam.json",
        beam_model().into_bytes(),
    );
    for (name, image) in [
        ("saucer_hull", hull_texture()),
        ("saucer_rim", rim_texture()),
        ("saucer_dome", dome_texture()),
        ("saucer_underside", underside_texture()),
        ("saucer_beam", beam_texture()),
    ] {
        pack.add(
            format!("assets/voidmap/textures/item/{name}.png"),
            image.to_png(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pointer::CLIENT_SNAP_DISTANCE;

    const EYE: [f64; 3] = [10.0, 65.62, -4.0];
    const LOOK: [f32; 2] = [170.0, 12.0];

    fn scenes() -> [Scene; 2] {
        [Part::Intro, Part::Outro].map(|part| Scene::new(part, EYE, LOOK))
    }

    fn apart(a: [f64; 3], b: [f64; 3]) -> f64 {
        (0..3)
            .map(|axis| (a[axis] - b[axis]).powi(2))
            .sum::<f64>()
            .sqrt()
    }

    /// Léo's three to five seconds, at twenty ticks each.
    #[test]
    fn a_film_stays_short() {
        for scene in scenes() {
            assert!((60..=110).contains(&scene.length()));
        }
    }

    /// A client draws the inside of a head, and there would be two bodies around the eye.
    #[test]
    fn the_body_is_never_shown_with_the_camera_in_its_head() {
        for scene in scenes() {
            for tick in 0..=scene.length() {
                let (camera, _) = Track(scene).at(f64::from(tick));
                if scene.body(tick).is_some() {
                    assert!(apart(camera, EYE) > 0.4, "{:?} {tick}", scene.part());
                }
            }
        }
    }

    #[test]
    fn a_film_seen_from_elsewhere_starts_and_ends_the_same() {
        for turn in Scene::turns() {
            let view = View { turn, lift: 2.0 };
            let outro = Scene::new(Part::Outro, EYE, LOOK).seen_from(view);
            assert_eq!(Track(outro).at(f64::from(OUTRO_TICKS)), (EYE, LOOK));
            let intro = Scene::new(Part::Intro, EYE, LOOK).seen_from(view);
            assert_eq!(Track(intro).at(0.0), (EYE, LOOK));
        }
    }

    /// A look down from over the head would see it go.
    #[test]
    fn the_head_is_out_of_the_frame_when_the_body_goes() {
        let look = [170.0, 70.0];
        let scene = Scene::new(Part::Outro, EYE, look);
        let tick = outro::BODY_GONE;
        assert!(Track(scene).at(f64::from(tick)).1[1] < 40.0);
    }

    #[test]
    fn the_camera_ends_in_the_eye_on_the_look_of_the_player() {
        let scene = Scene::new(Part::Outro, EYE, LOOK);
        assert_eq!(Track(scene).at(f64::from(OUTRO_TICKS)), (EYE, LOOK));
    }

    #[test]
    fn the_body_is_back_on_its_feet_when_the_camera_comes_down() {
        let scene = Scene::new(Part::Outro, EYE, LOOK);
        let body = scene.body(outro::BODY_GONE - 1).unwrap();
        assert_eq!(body.at, scene.feet());
        assert_eq!([body.yaw, body.pitch], LOOK);
        assert_eq!(body.stance, Stance::Stand);
    }

    /// A client swings the limbs from how far a body goes along the ground in a tick.
    #[test]
    fn a_body_in_the_beam_always_swings_its_limbs() {
        let scene = Scene::new(Part::Intro, EYE, LOOK);
        let flat = |at: [f64; 3]| [at[0], 0.0, at[2]];
        for tick in intro::LIFT..intro::TAKEN - 1 {
            let [from, to] = [tick, tick + 1].map(|tick| scene.body(tick).unwrap().at);
            assert!(apart(flat(from), flat(to)) > 0.2, "{tick}");
        }
    }

    #[test]
    fn a_body_goes_all_the_way_up_to_the_hull() {
        let scene = Scene::new(Part::Intro, EYE, LOOK);
        let last = scene.body(intro::TAKEN - 1).unwrap();
        assert!(last.at[1] > scene.feet()[1] + HOVER - HULL_BELOW - 0.5);
    }

    /// A stop at each key is what Léo saw as small stutters.
    #[test]
    fn a_way_goes_through_a_key_without_a_change_of_speed() {
        let key = |tick, x| Key::new(tick, [x, 0.0, 0.0], [0.0, 0.0]);
        let keys = [key(0, 0.0), key(10, 10.0), key(30, 40.0), key(40, 40.0)];
        let x = |tick: f64| along(&keys, tick).0[0];
        let (before, after) = (x(10.0) - x(9.9), x(10.1) - x(10.0));
        assert!(
            before > 0.05 && (before - after).abs() < 0.01,
            "{before} {after}"
        );
        assert_eq!(x(10.0), 10.0);
    }

    #[test]
    fn a_way_never_overshoots_a_key() {
        let key = |tick, x| Key::new(tick, [x, 0.0, 0.0], [0.0, 0.0]);
        let keys = [key(0, 0.0), key(5, 30.0), key(30, 31.0), key(40, 31.0)];
        for tick in 0..=400 {
            let x = along(&keys, f64::from(tick) / 10.0).0[0];
            assert!((-1e-9..=31.0 + 1e-9).contains(&x), "{tick} {x}");
        }
    }

    /// The camera of a film moves on from one tick to the next by about as much as the last
    /// time, whichever part of the film it is in.
    #[test]
    fn the_camera_of_a_film_never_changes_speed_all_at_once() {
        for scene in scenes() {
            let at = |tick: u32| Track(scene).at(f64::from(tick)).0;
            for tick in 1..scene.length() {
                let [before, after] =
                    [apart(at(tick - 1), at(tick)), apart(at(tick), at(tick + 1))];
                assert!((before - after).abs() < 0.3, "{:?} {tick}", scene.part());
            }
        }
    }

    /// Between two places a display glides, up to where a client gives up and jumps.
    #[test]
    fn the_saucer_never_goes_far_enough_for_a_client_to_jump() {
        for scene in scenes() {
            for tick in 0..scene.length() {
                let [from, to] = [tick, tick + 5].map(|tick| scene.saucer(tick).0);
                assert!(apart(from, to) < CLIENT_SNAP_DISTANCE);
            }
        }
    }

    #[test]
    fn the_beam_only_shines_with_the_saucer_over_the_player() {
        for scene in scenes() {
            for tick in (0..scene.length()).filter(|tick| scene.beam(*tick)) {
                let (at, _) = scene.saucer(tick);
                assert!(apart([at[0], 0.0, at[2]], [EYE[0], 0.0, EYE[2]]) < 0.01);
            }
        }
    }

    #[test]
    fn every_cue_of_a_film_comes_up_once() {
        for scene in scenes() {
            let cues: Vec<_> = (0..scene.length())
                .filter_map(|tick| scene.cue(tick))
                .collect();
            assert_eq!(cues.len(), 5);
            for (index, cue) in cues.iter().enumerate() {
                assert!(!cues[index + 1..].contains(cue));
            }
        }
    }

    #[test]
    fn the_models_only_name_textures_of_the_pack() {
        let mut pack = ResourcePack::new("test");
        install(&mut pack);
        for name in ["hull", "rim", "dome", "underside", "beam"] {
            assert!(pack.contains(&format!("assets/voidmap/textures/item/saucer_{name}.png")));
        }
        for model in [SAUCER_MODEL, BEAM_MODEL] {
            let name = model.strip_prefix("voidmap:").unwrap();
            assert!(pack.contains(&format!("assets/voidmap/items/{name}.json")));
        }
    }
}
