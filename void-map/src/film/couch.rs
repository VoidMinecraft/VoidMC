//! The clouds open and a sofa drops out of the sky, the way a cartoon starts: it comes down
//! with the player on it, or comes for them and flies off with them.

use voidmc::{Billboard, DisplayTransform};
use voidmc_protocol::clientbound::Particle;
use voidmc_ui::{Image, ResourcePack};

use super::{
    Block, Body, Burst, Noise, Part, Pose, Prop, Scene, Script, Stance, add_model, add_texture,
    along, block, blocks_model, eased, hop, noise, painted, palette, sheet_model,
};

const INTRO_TICKS: u32 = 108;
const OUTRO_TICKS: u32 = 110;

/// The sofa, at the scale of a model of one block.
const SCALE: f32 = 1.7;
/// How far the seat is over the ground, which is where a rider goes.
const SEAT_HEIGHT: f64 = 0.55;
/// How far the bottom of the model lies under the middle of an item, at scale 1.
const BOTTOM: f32 = 0.5;
/// Ahead of the player, facing them and the camera behind them.
const AHEAD: f64 = 1.9;
/// How low it gets when it lands, as a share of its height, and wider by as much.
const SQUASH: f32 = 0.62;
/// How high the clouds are, how big, and how far each goes to open.
const CLOUD_HEIGHT: f64 = 24.0;
const CLOUD_WIDTH: f32 = 34.0;
const CLOUD_TALL: f32 = 22.0;
const CLOUD_HALF: f64 = 13.0;
const CLOUD_OPEN: f64 = 9.0;
/// Where it comes from and goes to, over the feet.
const TOP: f64 = 48.0;
/// What drops quickly has to stop where it says: a short glide.
const GLIDE: u8 = 2;
const CLOUD_GLIDE: u8 = 4;

/// Its moments, for each part.
#[derive(Clone, Copy)]
struct Beats {
    clouds_come: u32,
    clouds_open: u32,
    falls: u32,
    lands: u32,
    /// The end of each of the two bounces.
    bounced: [u32; 2],
    /// It sinks on its legs, and goes.
    crouches: u32,
    leaves: u32,
    gone: u32,
    clouds_shut: u32,
}

const INTRO: Beats = Beats {
    clouds_come: 12,
    clouds_open: 24,
    falls: 32,
    lands: 50,
    bounced: [60, 66],
    crouches: 82,
    leaves: 88,
    gone: 108,
    clouds_shut: 98,
};

const OUTRO: Beats = Beats {
    clouds_come: 0,
    clouds_open: 6,
    falls: 14,
    lands: 38,
    bounced: [50, 58],
    crouches: 76,
    leaves: 80,
    gone: 98,
    clouds_shut: 90,
};

/// How long the clouds take to come, to open and to shut.
const CLOUD_TICKS: u32 = 10;
/// How high it bounces, the first time and the second.
const BOUNCES: [f64; 2] = [5.0, 1.4];
/// How long it stays flat from a landing.
const SQUASHED: u32 = 5;

/// The intro: the body comes into view, looks up, and hops on.
const BODY: u32 = 11;
const HOPS_ON: u32 = 70;
const SITS: u32 = 78;
/// The outro: the body gets off and comes round.
const HOPS_OFF: u32 = 66;
const OFF: u32 = 74;
const CALM: u32 = 92;
const NEARING: u32 = 86;
const OVER_THE_HEAD: u32 = 96;
const BODY_GONE: u32 = OUTRO_TICKS - super::BODY_LEAVES;

const LEFT_CLOUD: usize = 0;
const RIGHT_CLOUD: usize = 1;
const SOFA: usize = 2;

const PROPS: [Prop; 3] = [
    Prop {
        model: "voidmap:cloud",
        vanilla: "minecraft:white_wool",
        glide: CLOUD_GLIDE,
        morph: 3,
        billboard: Some(Billboard::Center),
        reach: [36.0, 36.0],
    },
    Prop {
        model: "voidmap:cloud",
        vanilla: "minecraft:white_wool",
        glide: CLOUD_GLIDE,
        morph: 3,
        billboard: Some(Billboard::Center),
        reach: [36.0, 36.0],
    },
    Prop {
        model: "voidmap:sofa",
        vanilla: "minecraft:green_bed",
        glide: GLIDE,
        morph: 2,
        billboard: None,
        reach: [5.0, 5.0],
    },
];

pub(super) struct Couch;

fn beats(scene: &Scene) -> Beats {
    match scene.part {
        Part::Intro => INTRO,
        Part::Outro => OUTRO,
    }
}

/// How high the seat is over where it rests.
fn height(scene: &Scene, tick: u32) -> f64 {
    let Beats {
        falls,
        lands,
        bounced: [first, second],
        leaves,
        gone,
        ..
    } = beats(scene);
    match tick {
        _ if tick < lands => TOP * f64::from(lands - tick.max(falls)) / f64::from(lands - falls),
        _ if tick < first => hop(tick, lands, first, BOUNCES[0]),
        _ if tick < second => hop(tick, first, second, BOUNCES[1]),
        _ if tick < leaves => 0.0,
        _ => TOP * (f64::from(tick - leaves) / f64::from(gone - leaves)).powi(2),
    }
}

/// How flat it is, from 1 on.
fn squash(scene: &Scene, tick: u32) -> f32 {
    let Beats {
        lands,
        bounced: [first, second],
        crouches,
        leaves,
        ..
    } = beats(scene);
    let flat = |from: u32| {
        if (from..from + SQUASHED).contains(&tick) {
            1.0 - eased(tick, from, from + SQUASHED) as f32
        } else {
            0.0
        }
    };
    let landed = flat(lands).max(flat(first) * 0.7).max(flat(second) * 0.4);
    let crouched = if (crouches..leaves).contains(&tick) {
        eased(tick, crouches, leaves) as f32
    } else {
        0.0
    };
    1.0 - (1.0 - SQUASH) * landed.max(crouched)
}

/// The seat at some tick.
fn seat(scene: &Scene, tick: u32) -> [f64; 3] {
    scene.off_feet(-AHEAD, 0.0, SEAT_HEIGHT + height(scene, tick))
}

/// Which way a body on it faces: to the camera.
fn facing(scene: &Scene) -> f32 {
    scene.look[0] + scene.view.turn + 180.0
}

/// The clouds, from their middle: how far each is from it, and how big.
fn clouds(scene: &Scene, tick: u32) -> (f64, f32) {
    let Beats {
        clouds_come,
        clouds_open,
        clouds_shut,
        ..
    } = beats(scene);
    let open = eased(tick, clouds_open, clouds_open + CLOUD_TICKS);
    let shut = eased(tick, clouds_shut, clouds_shut + CLOUD_TICKS);
    let size = match scene.part {
        Part::Intro => eased(tick, clouds_come, clouds_come + CLOUD_TICKS),
        Part::Outro => 1.0,
    };
    (
        CLOUD_HALF + CLOUD_OPEN * open * (1.0 - shut),
        size as f32 * (1.0 - eased(tick, clouds_shut + 4, clouds_shut + 10)) as f32,
    )
}

/// What a client lights is what faces the sky, and a sheet turned to a camera under it would
/// come out grey. Turned inside out by its depth, it takes the light of its other side.
fn lit_from(scene: &Scene) -> f32 {
    match scene.part {
        Part::Intro => -1.0,
        Part::Outro => 1.0,
    }
}

fn seated(scene: &Scene, tick: u32) -> Body {
    Body {
        at: seat(scene, tick),
        yaw: facing(scene),
        pitch: 0.0,
        stance: Stance::Stand,
        seat: Some(SOFA),
    }
}

/// On the way between the feet and the seat, as a share from the feet.
fn hopping(scene: &Scene, tick: u32, share: f64, from: u32, to: u32) -> Body {
    let feet = scene.feet();
    let on = seat(scene, tick);
    let mut at: [f64; 3] = std::array::from_fn(|axis| feet[axis] + (on[axis] - feet[axis]) * share);
    at[1] += hop(tick, from, to, 1.4);
    Body {
        at,
        yaw: scene.look[0],
        pitch: 0.0,
        stance: Stance::Stand,
        seat: None,
    }
}

fn fetched(scene: &Scene, tick: u32) -> Option<Body> {
    if tick < BODY {
        return None;
    }
    if tick >= SITS {
        return Some(seated(scene, tick));
    }
    if tick >= HOPS_ON {
        let share = eased(tick, HOPS_ON, SITS);
        return Some(hopping(scene, tick, share, HOPS_ON, SITS));
    }
    let up = eased(tick, 16, 26) * (1.0 - eased(tick, INTRO.lands, INTRO.lands + 6));
    let [_, pitch] = scene.look;
    let stance = if (INTRO.falls..INTRO.lands).contains(&tick) {
        Stance::Shiver
    } else {
        Stance::Stand
    };
    Some(scene.standing(pitch + (-60.0 - pitch) * up as f32, stance))
}

fn dropped(scene: &Scene, tick: u32) -> Option<Body> {
    if !(OUTRO.falls..BODY_GONE).contains(&tick) {
        return None;
    }
    if tick < HOPS_OFF {
        return Some(seated(scene, tick));
    }
    if tick < OFF {
        let share = 1.0 - eased(tick, HOPS_OFF, OFF);
        return Some(hopping(scene, tick, share, HOPS_OFF, OFF));
    }
    Some(scene.coming_round(tick, OFF, CALM))
}

fn dust(at: [f64; 3], count: i32) -> Burst {
    Burst {
        particle: Particle::Poof,
        at,
        spread: [2.0, 0.2, 1.4],
        speed: 0.12,
        count,
    }
}

impl Script for Couch {
    fn length(&self, part: Part) -> u32 {
        match part {
            Part::Intro => INTRO_TICKS,
            Part::Outro => OUTRO_TICKS,
        }
    }

    fn camera(&self, scene: &Scene, tick: f64) -> ([f64; 3], [f32; 2]) {
        let middle = |up| scene.off_feet(-AHEAD / 2.0, 0.0, up);
        match scene.part {
            Part::Intro => {
                let [eye, over_head] = scene.leaving_the_eye();
                let low = scene.raised(12.0, 5.0, 2.5);
                let watch = scene.raised(8.0, 3.5, 2.6);
                let wider = scene.raised(9.0, 4.5, 3.0);
                let keys = [
                    eye,
                    over_head,
                    scene.watching(30, low, middle(8.0)),
                    scene.watching(58, watch, middle(1.2)),
                    scene.watching(86, wider, middle(2.0)),
                    scene.watching(INTRO_TICKS, scene.raised(6.0, 3.0, 8.0), middle(TOP)),
                ];
                along(&keys, tick)
            }
            Part::Outro => {
                let sky = scene.sky_start();
                let wide = scene.raised(9.0, 4.5, 3.0);
                let turned = scene.raised(7.5, 2.5, 2.8);
                let nearing = scene.raised(5.5, 1.6, 3.4);
                let [over_head, over_eye, eye] = scene.back_in_the_eye(OVER_THE_HEAD, OUTRO_TICKS);
                let keys = [
                    sky,
                    super::Key { tick: 12, ..sky },
                    scene.watching(40, wide, middle(1.5)),
                    scene.watching(66, turned, middle(1.5)),
                    scene.watching(NEARING, nearing, middle(2.4)),
                    over_head,
                    over_eye,
                    eye,
                ];
                along(&keys, tick)
            }
        }
    }

    fn stops(&self, scene: &Scene) -> Vec<[f64; 3]> {
        vec![
            scene.off_feet(12.0, 5.0, 2.5),
            scene.off_feet(8.0, 3.5, 2.6),
            scene.off_feet(5.5, 1.6, 2.6),
            scene.off_feet(2.8, 0.8, 1.3),
        ]
    }

    fn props(&self) -> &'static [Prop] {
        &PROPS
    }

    fn pose(&self, scene: &Scene, prop: usize, tick: u32) -> Option<Pose> {
        let Beats {
            falls,
            gone,
            clouds_come,
            clouds_shut,
            ..
        } = beats(scene);
        match prop {
            SOFA => (falls.saturating_sub(1)..gone).contains(&tick).then(|| {
                let flat = squash(scene, tick);
                let drop = SEAT_HEIGHT as f32;
                Pose {
                    at: seat(scene, tick),
                    // A client turns an item display to face away from where it looks.
                    look: [facing(scene) + 180.0, 0.0],
                    shape: DisplayTransform::default()
                        .scale(SCALE * (2.0 - flat), SCALE * flat, SCALE * (2.0 - flat))
                        .translation(0.0, BOTTOM * SCALE * flat - drop, 0.0),
                }
            }),
            LEFT_CLOUD | RIGHT_CLOUD => {
                if !(clouds_come..clouds_shut + 10).contains(&tick) {
                    return None;
                }
                let (off, size) = clouds(scene, tick);
                let side = if prop == LEFT_CLOUD { -off } else { off };
                Some(Pose {
                    at: scene.off_feet(-AHEAD, side, CLOUD_HEIGHT),
                    look: [scene.look[0] + scene.view.turn, 0.0],
                    shape: DisplayTransform::default().scale(
                        CLOUD_WIDTH * size.max(0.01),
                        CLOUD_TALL * size.max(0.01),
                        lit_from(scene),
                    ),
                })
            }
            _ => None,
        }
    }

    fn body(&self, scene: &Scene, tick: u32) -> Option<Body> {
        match scene.part {
            Part::Intro => fetched(scene, tick),
            Part::Outro => dropped(scene, tick),
        }
    }

    fn noise(&self, scene: &Scene, tick: u32) -> Option<Noise> {
        let Beats {
            clouds_open,
            falls,
            lands,
            bounced: [first, _],
            leaves,
            ..
        } = beats(scene);
        match tick {
            _ if tick == clouds_open => noise("entity.phantom.flap", 0.6),
            _ if tick == falls => noise("entity.arrow.shoot", 0.5),
            _ if tick == lands => noise("entity.player.big_fall", 0.8),
            _ if tick == first => noise("entity.slime.jump", 0.7),
            _ if tick == leaves => noise("entity.firework_rocket.launch", 1.2),
            _ => None,
        }
    }

    fn bursts(&self, scene: &Scene, tick: u32) -> Vec<Burst> {
        let Beats {
            lands,
            bounced: [first, second],
            leaves,
            ..
        } = beats(scene);
        let ground = scene.off_feet(-AHEAD, 0.0, 0.2);
        match tick {
            _ if tick == lands => vec![dust(ground, 60)],
            _ if tick == first => vec![dust(ground, 24)],
            _ if tick == second => vec![dust(ground, 10)],
            // Not with the camera about to go through it.
            _ if tick == leaves && scene.part == Part::Intro => vec![dust(ground, 30)],
            _ => Vec::new(),
        }
    }

    fn install(&self, pack: &mut ResourcePack) {
        add_texture(pack, "sofa", &palette(&SOFA_COLOURS));
        add_texture(pack, "cloud", &cloud_texture());
        add_model(pack, "sofa", blocks_model("sofa", &sofa_blocks()));
        add_model(pack, "cloud", sheet_model("cloud"));
    }
}

const WOOD: usize = 0;
const FRAME: usize = 1;
const CUSHION: usize = 2;
const PIPING: usize = 3;
const SOFA_COLOURS: [[u8; 4]; 4] = [
    [112, 72, 38, 255],
    [28, 104, 100, 255],
    [58, 160, 146, 255],
    [236, 196, 92, 255],
];

/// From the feet up, with the seat towards the south.
fn sofa_blocks() -> Vec<Block> {
    let mut blocks = Vec::new();
    for (x, z) in [(-7.0, 1.0), (21.0, 1.0), (-7.0, 13.0), (21.0, 13.0)] {
        blocks.push(block([x, 0.0, z], [x + 2.0, 2.0, z + 2.0], WOOD));
    }
    blocks.push(block([-8.0, 2.0, 0.0], [24.0, 6.0, 16.0], FRAME));
    blocks.push(block([-8.0, 6.0, 0.0], [24.0, 19.0, 4.0], FRAME));
    blocks.push(block([-5.0, 6.0, 4.0], [7.5, 9.0, 16.5], CUSHION));
    blocks.push(block([8.5, 6.0, 4.0], [21.0, 9.0, 16.5], CUSHION));
    blocks.push(block([-5.0, 9.0, 4.0], [7.5, 16.0, 6.0], CUSHION));
    blocks.push(block([8.5, 9.0, 4.0], [21.0, 16.0, 6.0], CUSHION));
    for x in [-8.0, 21.0] {
        blocks.push(block([x, 6.0, 0.0], [x + 3.0, 12.0, 16.0], FRAME));
        blocks.push(block([x - 0.5, 12.0, 0.0], [x + 3.5, 13.5, 16.5], PIPING));
    }
    blocks
}

/// Puffs of a cloud, as across and down from its middle, and how wide, on a scale of 1.
const PUFFS: [(f64, f64, f64); 9] = [
    (-0.62, 0.12, 0.3),
    (-0.3, -0.1, 0.42),
    (0.08, -0.22, 0.5),
    (0.46, -0.05, 0.4),
    (0.72, 0.16, 0.24),
    (-0.42, 0.34, 0.34),
    (-0.02, 0.38, 0.42),
    (0.38, 0.36, 0.36),
    (0.0, 0.05, 0.5),
];
const CLOUD_SIZE: (u32, u32) = (192, 128);

/// A cloud as a cartoon draws it: flat white, with an edge and some blue in its underside.
fn cloud_texture() -> Image {
    let (width, height) = CLOUD_SIZE;
    painted(width, height, |x, y| {
        let across = (f64::from(x) + 0.5) / f64::from(width) * 2.0 - 1.0;
        let down = (f64::from(y) + 0.5) / f64::from(height) * 2.0 - 1.0;
        let inside = PUFFS
            .iter()
            .map(|(px, py, size)| {
                let (dx, dy) = ((across - px) / size, (down - py) / (size * 1.3));
                1.0 - dx.hypot(dy)
            })
            .fold(f64::MIN, f64::max);
        match inside {
            _ if inside < 0.0 => [0, 0, 0, 0],
            _ if inside < 0.07 => [176, 200, 232, 255],
            _ if inside < 0.2 && down > 0.0 => [222, 232, 248, 255],
            _ => [255, 255, 255, 255],
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::film::Film;

    const EYE: [f64; 3] = [10.0, 65.62, -4.0];
    const LOOK: [f32; 2] = [170.0, 12.0];

    fn scene(part: Part) -> Scene {
        Scene::new(Film::Couch, part, EYE, LOOK)
    }

    #[test]
    fn it_only_touches_the_ground_where_it_lands_and_rests() {
        for scene in [scene(Part::Intro), scene(Part::Outro)] {
            let Beats {
                lands,
                bounced: [first, second],
                leaves,
                ..
            } = beats(&scene);
            assert_eq!(height(&scene, lands), 0.0);
            assert!(height(&scene, (lands + first) / 2) > 4.0);
            assert_eq!(height(&scene, first), 0.0);
            assert!(height(&scene, (first + second) / 2) > 1.0);
            assert_eq!(height(&scene, leaves), 0.0);
            assert!(height(&scene, lands - 1) > 0.0);
        }
    }

    #[test]
    fn a_bounce_never_goes_under_the_ground() {
        for scene in [scene(Part::Intro), scene(Part::Outro)] {
            for tick in 0..scene.length() {
                assert!(height(&scene, tick) >= 0.0, "{tick}");
            }
        }
    }

    #[test]
    fn it_lands_flat_and_comes_back_to_its_shape() {
        let scene = scene(Part::Outro);
        assert_eq!(squash(&scene, OUTRO.lands), SQUASH);
        assert_eq!(squash(&scene, OUTRO.bounced[1] + SQUASHED), 1.0);
    }

    #[test]
    fn the_clouds_are_open_for_the_fall() {
        for scene in [scene(Part::Intro), scene(Part::Outro)] {
            let Beats { falls, lands, .. } = beats(&scene);
            for tick in falls + 6..lands {
                assert!(
                    clouds(&scene, tick).0 > CLOUD_HALF + CLOUD_OPEN * 0.8,
                    "{tick}"
                );
            }
        }
    }

    #[test]
    fn the_player_gets_off_before_the_sofa_leaves() {
        let scene = scene(Part::Outro);
        const { assert!(OFF < OUTRO.crouches) };
        assert_eq!(scene.body(OFF).unwrap().seat, None);
    }
}
