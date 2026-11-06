//! A saucer takes the player away, and brings them back.

use std::f64::consts::PI;

use voidmc::{Billboard, DisplayTransform};
use voidmc_protocol::clientbound::Particle;
use voidmc_ui::{Image, ResourcePack};

use super::{
    Body, Burst, Key, Noise, Part, Pose, Prop, Scene, Script, Stance, add_model, add_texture,
    along, eased, noise, painted, sheet_model,
};
use crate::pointer::ease;

/// How long each film lasts, with the camera in the eye at one end and over the land at
/// the other.
const INTRO_TICKS: u32 = 104;
const OUTRO_TICKS: u32 = 110;

/// How far over the feet the saucer hangs while the beam is on.
const HOVER: f64 = 8.5;
/// How much of the height of the saucer lies under where it is placed.
const HULL_BELOW: f64 = 0.65;
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
/// How far around the beam a body swings as it goes.
const SWING_RADIUS: f64 = 0.75;
const RUN_RADIUS: f64 = super::RUN_RADIUS;
/// Degrees the saucer turns in a tick.
const SPIN: f32 = 14.0;
/// The saucer, at the scale of a model of one block across.
const SCALE: f32 = 3.4;
const BEAM_WIDTH: f32 = 6.5;
/// How long a display takes to get to a new place.
const GLIDE: u8 = crate::transition::FLIGHT_GLIDE_TICKS;

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
    pub const BODY_GONE: u32 = super::OUTRO_TICKS - crate::film::BODY_LEAVES;
}

const HULL_PROP: usize = 0;

const PROPS: [Prop; 2] = [
    Prop {
        model: "voidmap:saucer",
        vanilla: "minecraft:heavy_weighted_pressure_plate",
        glide: GLIDE,
        morph: 0,
        billboard: None,
        reach: [12.0, 6.0],
    },
    // A single sheet of light, which the client turns to the camera around its height.
    Prop {
        model: "voidmap:saucer_beam",
        vanilla: "minecraft:white_stained_glass_pane",
        glide: GLIDE,
        morph: 4,
        billboard: Some(Billboard::Vertical),
        reach: [10.0, 14.0],
    },
];

pub(super) struct Saucer;

/// Where the saucer goes off to: ahead of the player, into the sky.
fn exit(scene: &Scene) -> [f64; 3] {
    scene.off_feet(-90.0, 24.0, 62.0)
}

/// The place of the saucer, and how it leans, as yaw and pitch.
fn saucer_at(scene: &Scene, tick: u32) -> ([f64; 3], [f32; 2]) {
    let over = scene.off_feet(0.0, 0.0, HOVER);
    let far = scene.off_feet(-44.0, 10.0, 30.0);
    let gone = exit(scene);
    let high = scene.off_feet(-6.0, -20.0, 60.0);
    let flat = [0.0, 0.0];
    let keys: &[Key] = match scene.part {
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
    (at, [tick as f32 * SPIN, pitch])
}

/// Whether the saucer has anything to show yet, and still has.
fn saucer_shown(scene: &Scene, tick: u32) -> bool {
    match scene.part {
        Part::Intro => (intro::SAUCER_IN.saturating_sub(4)..intro::GONE).contains(&tick),
        Part::Outro => tick < outro::GONE,
    }
}

/// Whether the beam is on.
fn beam_on(scene: &Scene, tick: u32) -> bool {
    match scene.part {
        Part::Intro => (intro::BEAM_ON..intro::BEAM_OFF).contains(&tick),
        Part::Outro => (outro::BEAM_ON..outro::BEAM_OFF).contains(&tick),
    }
}

/// A sheet hanging from the saucer.
fn beam_shape(reach: f32) -> DisplayTransform {
    let length = HOVER as f32 - 0.4;
    DisplayTransform::default()
        .translation(0.0, -0.5 - length * reach / 2.0, 0.0)
        .scale(BEAM_WIDTH, length * reach, 1.0)
}

fn taken(scene: &Scene, tick: u32) -> Option<Body> {
    use intro::{BODY, LIFT, LOOKS_UP, PANIC, TAKEN, WHIRL};
    let [_, pitch] = scene.look;
    if !(BODY..TAKEN).contains(&tick) {
        return None;
    }
    if tick < PANIC {
        let up = eased(tick, LOOKS_UP, LOOKS_UP + 8) as f32;
        return Some(scene.standing(pitch + (-65.0 - pitch) * up, Stance::Stand));
    }
    if tick < LIFT {
        // Running on the spot, for all the good it does.
        let radius = RUN_RADIUS * eased(tick, PANIC, PANIC + 5);
        return Some(scene.swinging(tick, 0.0, radius, Stance::Shiver));
    }
    let share = f64::from(tick - LIFT) / f64::from(TAKEN - LIFT);
    let height = ease(share) * (HOVER - HULL_BELOW + 0.4);
    let swell = (PI * share).sin();
    let radius = RUN_RADIUS + (SWING_RADIUS - RUN_RADIUS) * swell;
    let stance = match tick {
        _ if tick >= WHIRL => Stance::Whirl,
        _ if ((tick - LIFT) / 9).is_multiple_of(2) => Stance::Flail,
        _ => Stance::Tumble,
    };
    Some(scene.swinging(tick, height, radius, stance))
}

fn brought_back(scene: &Scene, tick: u32) -> Option<Body> {
    use outro::{BODY_GONE, CALM, LANDED, OUT, STANDS};
    if !(OUT..BODY_GONE).contains(&tick) {
        return None;
    }
    if tick < LANDED {
        let share = f64::from(tick - OUT) / f64::from(LANDED - OUT);
        let height = (1.0 - ease(share)) * (HOVER - HULL_BELOW + 0.4);
        let radius = SWING_RADIUS * (PI * share).sin().sqrt();
        let stance = if ((tick - OUT) / 9).is_multiple_of(2) {
            Stance::Tumble
        } else {
            Stance::Flail
        };
        return Some(scene.swinging(tick, height, radius, stance));
    }
    if tick < STANDS {
        return Some(scene.standing(40.0, Stance::Cower));
    }
    Some(scene.coming_round(tick, STANDS, CALM))
}

impl Script for Saucer {
    fn length(&self, part: Part) -> u32 {
        match part {
            Part::Intro => INTRO_TICKS,
            Part::Outro => OUTRO_TICKS,
        }
    }

    fn camera(&self, scene: &Scene, tick: f64) -> ([f64; 3], [f32; 2]) {
        let middle = |up| scene.over_feet(up);
        match scene.part {
            Part::Intro => {
                use intro::{CLOSE, FOLLOWED, WATCHING};
                let watch = scene.raised(WATCH_BACK, WATCH_SIDE, WATCH_HEIGHT);
                let close = scene.raised(CLOSE_BACK, CLOSE_SIDE, 0.9);
                let followed = scene.raised(CLOSE_BACK, CLOSE_SIDE, 2.6);
                let chase = scene.off_feet(-12.0, 4.0, 30.0);
                let [eye, over_head] = scene.leaving_the_eye();
                let keys = [
                    eye,
                    over_head,
                    scene.watching(WATCHING, watch, middle(4.4)),
                    scene.watching(CLOSE, close, middle(3.0)),
                    scene.watching(FOLLOWED, followed, middle(HOVER)),
                    scene.watching(INTRO_TICKS, chase, exit(scene)),
                ];
                along(&keys, tick)
            }
            Part::Outro => {
                use outro::{NEARING, OVER_THE_HEAD, TURNED, WIDE};
                let wide = scene.raised(WIDE_BACK, WIDE_SIDE, WIDE_HEIGHT);
                let turned = scene.raised(TURNED_BACK, TURNED_SIDE, TURNED_HEIGHT);
                let nearing = scene.raised(NEARING_BACK, NEARING_SIDE, NEARING_HEIGHT);
                let [over_head, over_eye, eye] = scene.back_in_the_eye(OVER_THE_HEAD, OUTRO_TICKS);
                let keys = [
                    scene.sky_start(),
                    scene.watching(WIDE, wide, middle(WIDE_AIM)),
                    scene.watching(TURNED, turned, middle(WIDE_AIM)),
                    scene.watching(NEARING, nearing, middle(2.6)),
                    over_head,
                    over_eye,
                    eye,
                ];
                along(&keys, tick)
            }
        }
    }

    fn stops(&self, scene: &Scene) -> Vec<[f64; 3]> {
        match scene.part {
            Part::Intro => {
                let [x, y, z] = scene.off_feet(WATCH_BACK, WATCH_SIDE, WATCH_HEIGHT);
                let [feet_x, _, feet_z] = scene.feet();
                vec![
                    [x, y, z],
                    [(x + feet_x) / 2.0, y, (z + feet_z) / 2.0],
                    scene.off_feet(CLOSE_BACK, CLOSE_SIDE, 0.9),
                    scene.off_feet(CLOSE_BACK / 2.0, CLOSE_SIDE / 2.0, 0.9),
                ]
            }
            Part::Outro => vec![
                scene.off_feet(WIDE_BACK, WIDE_SIDE, WIDE_HEIGHT),
                scene.off_feet(TURNED_BACK, TURNED_SIDE, TURNED_HEIGHT),
                scene.off_feet(NEARING_BACK, NEARING_SIDE, NEARING_HEIGHT),
                scene.off_feet(NEARING_BACK / 2.0, NEARING_SIDE / 2.0, NEARING_HEIGHT / 2.0),
            ],
        }
    }

    fn props(&self) -> &'static [Prop] {
        &PROPS
    }

    fn pose(&self, scene: &Scene, prop: usize, tick: u32) -> Option<Pose> {
        if !saucer_shown(scene, tick) {
            return None;
        }
        let (at, look) = saucer_at(scene, tick);
        Some(match prop {
            HULL_PROP => Pose {
                at,
                look,
                shape: DisplayTransform::default().uniform_scale(SCALE),
            },
            _ => Pose {
                at,
                look: [0.0, 0.0],
                shape: beam_shape(if beam_on(scene, tick) { 1.0 } else { 0.0 }),
            },
        })
    }

    fn body(&self, scene: &Scene, tick: u32) -> Option<Body> {
        match scene.part {
            Part::Intro => taken(scene, tick),
            Part::Outro => brought_back(scene, tick),
        }
    }

    fn noise(&self, scene: &Scene, tick: u32) -> Option<Noise> {
        match scene.part {
            Part::Intro => match tick {
                intro::SAUCER_IN => noise("entity.phantom.swoop", 0.6),
                intro::BEAM_ON => noise("block.beacon.activate", 1.4),
                intro::LIFT => noise("entity.villager.hurt", 1.3),
                intro::TAKEN => noise("entity.enderman.teleport", 1.0),
                intro::DEPART => noise("entity.firework_rocket.launch", 0.6),
                _ => None,
            },
            Part::Outro => match tick {
                1 => noise("entity.phantom.swoop", 0.6),
                outro::BEAM_ON => noise("block.beacon.activate", 1.4),
                outro::OUT => noise("entity.villager.no", 1.2),
                outro::LANDED => noise("entity.player.big_fall", 1.0),
                outro::DEPART => noise("entity.firework_rocket.launch", 0.6),
                _ => None,
            },
        }
    }

    fn bursts(&self, scene: &Scene, tick: u32) -> Vec<Burst> {
        let mut bursts = Vec::new();
        // They hang for seconds, and on the way back the camera goes through where they are.
        let sparks =
            beam_on(scene, tick) && (scene.part == Part::Intro || tick < outro::SPARKS_END);
        if sparks {
            let height = f64::from(tick * 7 % 100) / 100.0 * HOVER;
            bursts.push(Burst {
                particle: Particle::EndRod,
                at: scene.over_feet(height),
                spread: [0.7; 3],
                speed: 0.0,
                count: 1,
            });
        }
        if scene.part == Part::Outro && tick == outro::LANDED {
            bursts.push(Burst {
                particle: Particle::Cloud,
                at: scene.over_feet(0.1),
                spread: [0.5, 0.1, 0.5],
                speed: 0.1,
                count: 6,
            });
        }
        bursts
    }

    fn install(&self, pack: &mut ResourcePack) {
        add_model(pack, "saucer", hull_model());
        add_model(pack, "saucer_beam", sheet_model("saucer_beam"));
        for (name, image) in [
            ("saucer_hull", hull_texture()),
            ("saucer_rim", rim_texture()),
            ("saucer_dome", dome_texture()),
            ("saucer_underside", underside_texture()),
            ("saucer_beam", beam_texture()),
        ] {
            add_texture(pack, name, &image);
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::film::Film;

    const EYE: [f64; 3] = [10.0, 65.62, -4.0];
    const LOOK: [f32; 2] = [170.0, 12.0];

    fn scene(part: Part) -> Scene {
        Scene::new(Film::Saucer, part, EYE, LOOK)
    }

    fn apart(a: [f64; 3], b: [f64; 3]) -> f64 {
        (0..3)
            .map(|axis| (a[axis] - b[axis]).powi(2))
            .sum::<f64>()
            .sqrt()
    }

    /// A client swings the limbs from how far a body goes along the ground in a tick.
    #[test]
    fn a_body_in_the_beam_always_swings_its_limbs() {
        let scene = scene(Part::Intro);
        let flat = |at: [f64; 3]| [at[0], 0.0, at[2]];
        for tick in intro::LIFT..intro::TAKEN - 1 {
            let [from, to] = [tick, tick + 1].map(|tick| scene.body(tick).unwrap().at);
            assert!(apart(flat(from), flat(to)) > 0.2, "{tick}");
        }
    }

    #[test]
    fn a_body_goes_all_the_way_up_to_the_hull() {
        let scene = scene(Part::Intro);
        let last = scene.body(intro::TAKEN - 1).unwrap();
        assert!(last.at[1] > scene.feet()[1] + HOVER - HULL_BELOW - 0.5);
    }

    #[test]
    fn the_beam_only_shines_with_the_saucer_over_the_player() {
        for scene in [scene(Part::Intro), scene(Part::Outro)] {
            for tick in (0..scene.length()).filter(|tick| beam_on(&scene, *tick)) {
                let (at, _) = saucer_at(&scene, tick);
                assert!(apart([at[0], 0.0, at[2]], [EYE[0], 0.0, EYE[2]]) < 0.01);
            }
        }
    }

    #[test]
    fn the_models_only_name_textures_of_the_pack() {
        let mut pack = ResourcePack::new("test");
        Saucer.install(&mut pack);
        for name in ["hull", "rim", "dome", "underside", "beam"] {
            assert!(pack.contains(&format!("assets/voidmap/textures/item/saucer_{name}.png")));
        }
    }
}
