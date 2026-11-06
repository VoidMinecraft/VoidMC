//! The way between the world and the map as a small film: a saucer takes the player away, a
//! shark eats them, a sofa flies off with them. Each [`Film`] has a [`Script`], which says what
//! happens at each tick of a [`Scene`], with no entities, and `plugin::film` plays any of them.
//! The clock is the flight of the camera, so that a film waits with it and stops with it.

mod couch;
mod saucer;
mod shark;

use bevy_ecs::prelude::Component;
use voidmc::{Billboard, DisplayTransform};
use voidmc_protocol::clientbound::Particle;
use voidmc_ui::{Image, ResourcePack};

use crate::pointer::{SPECTATOR_EYE_HEIGHT, ease, look_direction};
use crate::transition::SWAP_TICKS;

/// Straight down, as a look.
const DOWN: f32 = 90.0;
/// How far the camera goes up on its way to the map.
const SKY_HEIGHT: f64 = 34.0;
/// The way out of an eye, and back into it: straight up through the top of the head and
/// behind it from there. Coming along the look, the back of the head would fill the screen.
const OVER_THE_HEAD: f64 = 0.85;
/// Coming back, high enough for the head to go out at the bottom of the frame.
const HIGH_OVER_THE_HEAD: f64 = 2.0;
/// The last moment the camera looks ahead, with the body gone from under it.
const OVER_THE_EYE: f64 = 1.0;
/// The last ticks of an outro, when the camera drops into the eye.
const INTO_THE_EYE: u32 = 8;
/// The body goes this long before the end: a client draws the inside of a head.
const BODY_LEAVES: u32 = INTO_THE_EYE + 1;
const LEVEL_PITCH: f32 = 12.0;
/// When the camera is over the head, on its way out of the eye.
const OUT_OF_THE_HEAD: u32 = 12;
/// How far around a spot a body swings as it goes, and how fast. A client swings the legs
/// from how far an entity goes in a tick, and is at full swing from a quarter block.
const SWING_PER_TICK: f64 = 0.62;
/// The same on the ground, where it passes for running.
const RUN_RADIUS: f64 = 0.4;

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

/// How far along from one tick to the next, eased at both ends, and 0 or 1 outside.
fn eased(tick: u32, from: u32, to: u32) -> f64 {
    ease(((f64::from(tick) - f64::from(from)) / f64::from(to - from)).clamp(0.0, 1.0))
}

/// How high a bounce is at some tick, on a curve like a thrown thing's.
fn hop(tick: u32, from: u32, to: u32, height: f64) -> f64 {
    let share = (f64::from(tick) - f64::from(from)) / f64::from(to - from);
    height * (4.0 * share * (1.0 - share)).max(0.0)
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
    /// The prop it sits on, by its place in [`Scene::props`]. A client sits what rides.
    pub seat: Option<usize>,
}

/// Something to hear once.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Noise {
    pub name: &'static str,
    pub volume: f32,
    pub pitch: f32,
}

const fn noise(name: &'static str, pitch: f32) -> Option<Noise> {
    Some(Noise {
        name,
        volume: 2.0,
        pitch,
    })
}

/// Particles to show once.
#[derive(Debug, Clone, PartialEq)]
pub struct Burst {
    pub particle: Particle,
    pub at: [f64; 3],
    pub spread: [f32; 3],
    pub speed: f32,
    pub count: i32,
}

/// Something a film shows besides the body: an item display of the pack.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Prop {
    /// What an item takes to look like it, in its `item_model`.
    pub model: &'static str,
    /// What a client with no pack gets instead.
    pub vanilla: &'static str,
    /// How long it takes to get to a new place. It gets one each tick, as far ahead as that.
    pub glide: u8,
    /// How long it takes to change shape, the same way.
    pub morph: u16,
    /// How the client keeps turning it to the camera, if it does.
    pub billboard: Option<Billboard>,
    /// How wide and how high it reaches, for a client to tell when it is out of sight.
    pub reach: [f32; 2],
}

/// A prop at some tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub at: [f64; 3],
    pub look: [f32; 2],
    pub shape: DisplayTransform,
}

/// Which way around the player the film is shot, for the land to leave room for the camera.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct View {
    /// Degrees from the way the player looks.
    pub turn: f32,
    /// How far over its usual height the camera goes.
    pub lift: f64,
}

/// A film to go by. Each one is a [`Script`], and that is all there is to know of it here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Film {
    Saucer,
    Shark,
    Couch,
}

impl Film {
    pub const ALL: [Self; 3] = [Self::Saucer, Self::Shark, Self::Couch];

    fn script(self) -> &'static dyn Script {
        match self {
            Self::Saucer => &saucer::Saucer,
            Self::Shark => &shark::Shark,
            Self::Couch => &couch::Couch,
        }
    }
}

/// What makes a film. It starts in the eye on an intro and over the land on an outro, which
/// has to be right over the feet, as the side it is shot from is settled later. An outro ends
/// in the eye, with the body back on its feet before that.
trait Script: Sync {
    fn length(&self, part: Part) -> u32;
    fn camera(&self, scene: &Scene, tick: f64) -> ([f64; 3], [f32; 2]);
    /// Where the camera goes, for the land to be checked.
    fn stops(&self, scene: &Scene) -> Vec<[f64; 3]>;
    fn props(&self) -> &'static [Prop];
    /// A prop at some tick, if it is to be seen.
    fn pose(&self, scene: &Scene, prop: usize, tick: u32) -> Option<Pose>;
    /// The body at some tick, if it is to be seen. A client draws the inside of a head: there
    /// is none with the camera around the eye.
    fn body(&self, scene: &Scene, tick: u32) -> Option<Body>;
    /// What is heard right at this tick.
    fn noise(&self, scene: &Scene, tick: u32) -> Option<Noise>;
    /// What flies off at this tick.
    fn bursts(&self, scene: &Scene, tick: u32) -> Vec<Burst>;
    fn install(&self, pack: &mut ResourcePack);
}

/// One film, from where the player's eye is and how they look. All of it follows from that.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scene {
    film: Film,
    part: Part,
    eye: [f64; 3],
    look: [f32; 2],
    view: View,
}

impl Scene {
    pub fn new(film: Film, part: Part, eye: [f64; 3], look: [f32; 2]) -> Self {
        Self {
            film,
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
        self.film.script().stops(&around)
    }

    pub fn part(&self) -> Part {
        self.part
    }

    pub fn length(&self) -> u32 {
        self.film.script().length(self.part)
    }

    pub fn props(&self) -> &'static [Prop] {
        self.film.script().props()
    }

    pub fn pose(&self, prop: usize, tick: u32) -> Option<Pose> {
        self.film.script().pose(self, prop, tick)
    }

    pub fn body(&self, tick: u32) -> Option<Body> {
        self.film.script().body(self, tick)
    }

    pub fn noise(&self, tick: u32) -> Option<Noise> {
        self.film.script().noise(self, tick)
    }

    pub fn bursts(&self, tick: u32) -> Vec<Burst> {
        self.film.script().bursts(self, tick)
    }

    /// Where the player stands.
    pub fn feet(&self) -> [f64; 3] {
        let [x, y, z] = self.eye;
        [x, y - SPECTATOR_EYE_HEIGHT, z]
    }

    /// This far up from the feet.
    fn over_feet(&self, up: f64) -> [f64; 3] {
        let [x, y, z] = self.feet();
        [x, y + up, z]
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

    /// `up` blocks over the usual height of a camera, for the land to leave room.
    fn raised(&self, back: f64, side: f64, up: f64) -> [f64; 3] {
        self.off_feet(back, side, up + self.view.lift)
    }

    /// A key with the camera there, aimed at that.
    fn watching(&self, tick: u32, at: [f64; 3], target: [f64; 3]) -> Key {
        Key::new(tick, at, self.aim(at, target))
    }

    /// The way out of the eye. The look stays level over the head, whatever the player's:
    /// from there, a look down would have the head in frame.
    fn leaving_the_eye(&self) -> [Key; 2] {
        let [x, y, z] = self.eye;
        [
            Key::new(SWAP_TICKS, self.eye, self.look),
            Key::new(
                OUT_OF_THE_HEAD,
                [x, y + OVER_THE_HEAD, z],
                self.level_look(),
            ),
        ]
    }

    /// The way back into the eye, the last `ticks` of a film of this length. It comes over
    /// the head from high enough not to see it, and drops once the body is gone.
    fn back_in_the_eye(&self, over_the_head: u32, length: u32) -> [Key; 3] {
        let [x, y, z] = self.eye;
        let look = self.level_look();
        [
            Key::new(over_the_head, [x, y + HIGH_OVER_THE_HEAD, z], look),
            Key::new(length - INTO_THE_EYE, [x, y + OVER_THE_EYE, z], look),
            Key::new(length, self.eye, self.look),
        ]
    }

    fn level_look(&self) -> [f32; 2] {
        [self.look[0], self.look[1].min(LEVEL_PITCH)]
    }

    /// Up over the feet, looking down, where an outro has to start.
    fn sky_start(&self) -> Key {
        Key::new(0, self.over_feet(SKY_HEIGHT), [self.look[0], DOWN])
    }

    /// A body on its feet, by the look of the player.
    fn standing(&self, pitch: f32, stance: Stance) -> Body {
        Body {
            at: self.feet(),
            yaw: self.look[0],
            pitch,
            stance,
            seat: None,
        }
    }

    /// Going round a spot at some height, facing the way it goes.
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
            seat: None,
        }
    }

    /// Dizzy from one tick, and by the other back to how the player looks, for the camera to
    /// take over from it.
    fn coming_round(&self, tick: u32, from: u32, to: u32) -> Body {
        let calm = eased(tick, from, to);
        let staggering = self.swinging(tick, 0.0, RUN_RADIUS * (1.0 - calm), Stance::Shiver);
        let calm = calm as f32;
        let dizzy = ((f64::from(tick) * 0.55).sin() * 45.0) as f32 * (1.0 - calm);
        let [yaw, pitch] = self.look;
        Body {
            at: staggering.at,
            yaw: yaw + dizzy,
            pitch: pitch * calm + 12.0 * (1.0 - calm),
            stance: if calm < 1.0 {
                Stance::Shiver
            } else {
                Stance::Stand
            },
            seat: None,
        }
    }
}

/// A way of the camera, for `Flight` to fly: it starts and ends where the scene says.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Track(pub Scene);

impl Track {
    pub fn at(&self, tick: f64) -> ([f64; 3], [f32; 2]) {
        self.0.film.script().camera(&self.0, tick)
    }
}

/// A box of a model, in sixteenths of a block around the middle of the item, with a colour of
/// the film's palette for its sides, its top and its underside.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Block {
    from: [f32; 3],
    to: [f32; 3],
    sides: usize,
    top: usize,
    under: usize,
}

const fn block(from: [f32; 3], to: [f32; 3], colour: usize) -> Block {
    Block {
        from,
        to,
        sides: colour,
        top: colour,
        under: colour,
    }
}

/// A model made of boxes, each face a pixel of a palette, one colour after the other.
fn blocks_model(palette: &str, blocks: &[Block]) -> String {
    let face = |side: &str, colour: usize| {
        let at = colour as f32;
        let uv = format!("[{},0,{},1]", at + 0.25, at + 0.75);
        format!(r##""{side}":{{"texture":"#palette","uv":{uv}}}"##)
    };
    let element = |block: &Block| {
        let sides = ["north", "south", "east", "west"].map(|side| face(side, block.sides));
        format!(
            r#"{{"from":{:?},"to":{:?},"faces":{{{},{},{}}}}}"#,
            block.from,
            block.to,
            sides.join(","),
            face("up", block.top),
            face("down", block.under),
        )
    };
    let elements: Vec<_> = blocks.iter().map(element).collect();
    format!(
        r#"{{"textures":{{"particle":"voidmap:item/{palette}","palette":"voidmap:item/{palette}"}},"elements":[{}]}}"#,
        elements.join(",")
    )
}

/// A single sheet with a texture on both sides, for a billboard.
fn sheet_model(name: &str) -> String {
    let face = r##"{"texture":"#sheet","uv":[0,0,16,16]}"##;
    format!(
        r#"{{"textures":{{"particle":"voidmap:item/{name}","sheet":"voidmap:item/{name}"}},"elements":[{{"from":[0,0,8],"to":[16,16,8],"shade":false,"faces":{{"north":{face},"south":{face}}}}}]}}"#
    )
}

/// A row of colours, for [`blocks_model`]. A model takes its faces from a square of 16 pixels.
fn palette(colours: &[[u8; 4]]) -> Image {
    painted(16, 16, |x, _| {
        colours.get(x as usize).copied().unwrap_or([0, 0, 0, 0])
    })
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

/// An item with this model, by its name in the pack.
fn add_model(pack: &mut ResourcePack, name: &str, model: String) {
    let item = format!(r#"{{"model":{{"type":"minecraft:model","model":"voidmap:item/{name}"}}}}"#);
    pack.add(
        format!("assets/voidmap/items/{name}.json"),
        item.into_bytes(),
    );
    pack.add(
        format!("assets/voidmap/models/item/{name}.json"),
        model.into_bytes(),
    );
}

fn add_texture(pack: &mut ResourcePack, name: &str, image: &Image) {
    pack.add(
        format!("assets/voidmap/textures/item/{name}.png"),
        image.to_png(),
    );
}

/// Adds what the films are made of.
pub fn install(pack: &mut ResourcePack) {
    for film in Film::ALL {
        film.script().install(pack);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pointer::CLIENT_SNAP_DISTANCE;

    const EYE: [f64; 3] = [10.0, 65.62, -4.0];
    const LOOK: [f32; 2] = [170.0, 12.0];

    fn scenes() -> Vec<Scene> {
        Film::ALL
            .into_iter()
            .flat_map(|film| {
                [Part::Intro, Part::Outro].map(|part| Scene::new(film, part, EYE, LOOK))
            })
            .collect()
    }

    fn apart(a: [f64; 3], b: [f64; 3]) -> f64 {
        (0..3)
            .map(|axis| (a[axis] - b[axis]).powi(2))
            .sum::<f64>()
            .sqrt()
    }

    /// Léo's three to five seconds, at twenty ticks each, and a little more for the gags.
    #[test]
    fn a_film_stays_short() {
        for scene in scenes() {
            assert!((60..=115).contains(&scene.length()));
        }
    }

    /// A client draws the inside of a head, and there would be two bodies around the eye.
    #[test]
    fn the_body_is_never_shown_with_the_camera_in_its_head() {
        for scene in scenes() {
            for tick in 0..=scene.length() {
                let (camera, _) = Track(scene).at(f64::from(tick));
                if scene.body(tick).is_some() {
                    assert!(apart(camera, EYE) > 0.4, "{scene:?} {tick}");
                }
            }
        }
    }

    #[test]
    fn a_film_seen_from_elsewhere_starts_and_ends_the_same() {
        for film in Film::ALL {
            for turn in Scene::turns() {
                let view = View { turn, lift: 2.0 };
                let outro = Scene::new(film, Part::Outro, EYE, LOOK).seen_from(view);
                let length = f64::from(outro.length());
                assert_eq!(Track(outro).at(length), (EYE, LOOK));
                let intro = Scene::new(film, Part::Intro, EYE, LOOK).seen_from(view);
                assert_eq!(Track(intro).at(0.0), (EYE, LOOK));
            }
        }
    }

    /// The camera of an outro is there before the side it is shot from is settled.
    #[test]
    fn an_outro_starts_from_the_same_place_on_all_sides() {
        for film in Film::ALL {
            let scene = Scene::new(film, Part::Outro, EYE, LOOK);
            let start = Track(scene).at(0.0);
            for turn in Scene::turns() {
                let view = View { turn, lift: 3.0 };
                assert_eq!(Track(scene.seen_from(view)).at(0.0), start);
            }
        }
    }

    /// A look down from over the head would see it go.
    #[test]
    fn the_head_is_out_of_the_frame_when_the_body_goes() {
        for film in Film::ALL {
            let scene = Scene::new(film, Part::Outro, EYE, [170.0, 70.0]);
            let gone = (0..scene.length()).rfind(|tick| scene.body(*tick).is_some());
            let tick = gone.expect("a body");
            assert!(Track(scene).at(f64::from(tick + 1)).1[1] < 40.0, "{film:?}");
        }
    }

    #[test]
    fn the_body_is_back_on_its_feet_when_the_camera_comes_down() {
        for film in Film::ALL {
            let scene = Scene::new(film, Part::Outro, EYE, LOOK);
            let gone = (0..scene.length()).rfind(|tick| scene.body(*tick).is_some());
            let body = scene.body(gone.expect("a body")).expect("a body");
            assert_eq!(body.at, scene.feet(), "{film:?}");
            assert_eq!([body.yaw, body.pitch], LOOK, "{film:?}");
            assert_eq!(body.stance, Stance::Stand, "{film:?}");
            assert_eq!(body.seat, None, "{film:?}");
        }
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
                assert!((before - after).abs() < 0.35, "{scene:?} {tick}");
            }
        }
    }

    /// Between two places a display glides, up to where a client gives up and jumps.
    #[test]
    fn no_prop_goes_far_enough_for_a_client_to_jump() {
        for scene in scenes() {
            for (index, prop) in scene.props().iter().enumerate() {
                let glide = u32::from(prop.glide);
                for tick in 0..scene.length() {
                    if let (Some(now), Some(next)) =
                        (scene.pose(index, tick), scene.pose(index, tick + glide))
                    {
                        assert!(apart(now.at, next.at) < CLIENT_SNAP_DISTANCE, "{prop:?}");
                    }
                }
            }
        }
    }

    /// A rider needs something to ride.
    #[test]
    fn a_seat_is_there_for_as_long_as_the_body_sits_on_it() {
        for scene in scenes() {
            for tick in 0..scene.length() {
                let seat = scene.body(tick).and_then(|body| body.seat);
                if let Some(seat) = seat {
                    assert!(scene.pose(seat, tick).is_some(), "{scene:?} {tick}");
                }
            }
        }
    }

    #[test]
    fn nothing_is_heard_twice_in_a_row() {
        for scene in scenes() {
            let heard: Vec<_> = (0..scene.length())
                .filter_map(|tick| scene.noise(tick))
                .collect();
            assert!(heard.len() >= 4, "{scene:?}");
            assert!(heard.windows(2).all(|pair| pair[0] != pair[1]), "{scene:?}");
        }
    }

    #[test]
    fn the_props_only_name_models_of_the_pack() {
        let mut pack = ResourcePack::new("test");
        install(&mut pack);
        for scene in scenes() {
            for prop in scene.props() {
                let name = prop.model.strip_prefix("voidmap:").unwrap();
                assert!(pack.contains(&format!("assets/voidmap/items/{name}.json")));
                assert!(pack.contains(&format!("assets/voidmap/models/item/{name}.json")));
            }
        }
    }
}
