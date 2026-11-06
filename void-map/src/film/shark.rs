//! A shark eats the player, and spits them back out. On dry land too: the ground turns into a
//! pool out of a cartoon, a fin goes round in it, and what comes out of there goes back there.

use std::f64::consts::TAU;

use voidmc::DisplayTransform;
use voidmc_protocol::clientbound::Particle;
use voidmc_ui::{Image, ResourcePack};

use super::{
    Block, Body, Burst, Key, Noise, Part, Pose, Prop, Scene, Script, Stance, add_model,
    add_texture, along, block, blocks_model, eased, hop, noise, painted, palette,
};
use crate::pointer::look_direction;

const INTRO_TICKS: u32 = 110;
const OUTRO_TICKS: u32 = 114;

/// How wide the pool gets, and how far over the ground it lies not to flicker.
const POOL_RADIUS: f64 = 5.0;
const POOL_HEIGHT: f64 = 0.08;
/// On the way back it opens ahead of the player, who gets thrown out of it to their place.
const POOL_AHEAD: f64 = 4.5;
/// How fast the fin goes round, in radians a tick, and how far from the middle.
const FIN_TURN: f64 = 0.17;
const FIN_FROM: f64 = 4.2;
const FIN_TO: f64 = 2.2;
const FIN_RISE: f64 = 1.5;
const FIN_RISE_TICKS: u32 = 6;
/// The shark, at the scale of a model of one block, and how far its mouth is from its middle.
const SCALE: f32 = 2.6;
const MOUTH: f64 = 3.4;
/// How wide its mouth opens, in degrees.
const GAPE: f32 = 58.0;
/// Where the lower jaw hangs from, in blocks from the middle of the model.
const HINGE: [f32; 3] = [0.0, -0.06, 0.62];
/// A quick thing does not glide for long, or its leap would be rounded off.
const GLIDE: u8 = 3;

/// Its moments, for each part.
#[derive(Clone, Copy)]
struct Beats {
    pool_open: u32,
    fin: u32,
    fin_gone: u32,
    leap: u32,
    /// The mouth is on the player, or on its way to throw them.
    bite: u32,
    /// Back in the pool.
    splash: u32,
    pool_shut: u32,
}

const INTRO: Beats = Beats {
    pool_open: 14,
    fin: 24,
    fin_gone: 64,
    leap: 70,
    bite: 80,
    splash: 98,
    pool_shut: 100,
};

const OUTRO: Beats = Beats {
    pool_open: 12,
    fin: 18,
    fin_gone: 46,
    leap: 50,
    bite: 60,
    splash: 78,
    pool_shut: 82,
};

/// How long the pool takes to open and to close.
const POOL_TICKS: u32 = 12;
/// The outro: the body comes out, lands, and is on its feet.
const THROWN: u32 = 62;
const LANDED: u32 = 76;
const STANDS: u32 = 80;
const CALM: u32 = 96;
const NEARING: u32 = 92;
const OVER_THE_HEAD: u32 = 102;
const BODY_GONE: u32 = OUTRO_TICKS - super::BODY_LEAVES;
/// The intro: the body comes into view, and goes down.
const BODY: u32 = 11;
const SHAKES: u32 = 40;
const SEIZED: u32 = 76;
const SWALLOWED: u32 = 81;
const BURP: u32 = 104;

const POOL: usize = 0;
const FIN: usize = 1;
const BODY_OF_SHARK: usize = 2;
const JAW: usize = 3;

const fn prop(model: &'static str, vanilla: &'static str, reach: [f32; 2]) -> Prop {
    Prop {
        model,
        vanilla,
        glide: GLIDE,
        morph: 2,
        billboard: None,
        reach,
    }
}

const PROPS: [Prop; 4] = [
    prop("voidmap:shark_pool", "minecraft:blue_carpet", [12.0, 1.0]),
    prop("voidmap:shark_fin", "minecraft:gray_banner", [3.0, 3.0]),
    prop("voidmap:shark", "minecraft:gray_concrete", [10.0, 10.0]),
    prop(
        "voidmap:shark_jaw",
        "minecraft:white_concrete",
        [10.0, 10.0],
    ),
];

pub(super) struct Shark;

fn beats(scene: &Scene) -> Beats {
    match scene.part {
        Part::Intro => INTRO,
        Part::Outro => OUTRO,
    }
}

/// The middle of the pool.
fn centre(scene: &Scene) -> [f64; 3] {
    match scene.part {
        Part::Intro => scene.over_feet(0.0),
        Part::Outro => scene.off_feet(-POOL_AHEAD, 0.0, 0.0),
    }
}

fn pool_radius(scene: &Scene, tick: u32) -> f64 {
    let Beats {
        pool_open,
        pool_shut,
        ..
    } = beats(scene);
    let open = eased(tick, pool_open, pool_open + POOL_TICKS);
    let shut = eased(tick, pool_shut, pool_shut + POOL_TICKS);
    POOL_RADIUS * open * (1.0 - shut)
}

/// The fin, with the angle it has got to.
fn fin_at(scene: &Scene, tick: u32) -> Option<([f64; 3], f64)> {
    let Beats { fin, fin_gone, .. } = beats(scene);
    if !(fin..fin_gone).contains(&tick) {
        return None;
    }
    let share = f64::from(tick - fin) / f64::from(fin_gone - fin);
    let angle = f64::from(tick) * FIN_TURN + f64::from(scene.look[0]).to_radians();
    let radius = FIN_FROM + (FIN_TO - FIN_FROM) * share;
    let up = eased(tick, fin, fin + FIN_RISE_TICKS)
        * (1.0 - eased(tick, fin_gone - FIN_RISE_TICKS, fin_gone));
    let [x, y, z] = centre(scene);
    Some((
        [
            x + angle.cos() * radius,
            y + (up - 1.0) * FIN_RISE,
            z + angle.sin() * radius,
        ],
        angle,
    ))
}

/// Which way it leaps, across the view.
fn across(scene: &Scene) -> f32 {
    scene.look[0] + scene.view.turn + 90.0
}

/// The mouth at some tick and how the shark leans, as a look.
fn mouth(scene: &Scene, tick: u32) -> ([f64; 3], [f32; 2]) {
    let Beats {
        leap, bite, splash, ..
    } = beats(scene);
    let top = bite + 7;
    // Along the way it leaps, and up: both follow one curve, as the pitch does.
    let keys = [
        Key::new(leap, [-4.5, -8.0, 0.0], [0.0, -80.0]),
        Key::new(bite, [-0.6, 1.2, 0.0], [0.0, -72.0]),
        Key::new(top, [1.6, 6.5, 0.0], [0.0, 0.0]),
        Key::new(splash, [3.6, -7.0, 0.0], [0.0, 80.0]),
    ];
    let ([along_way, up, _], [_, pitch]) = along(&keys, f64::from(tick));
    let yaw = across(scene);
    let way = look_direction(yaw, 0.0);
    let [x, y, z] = centre(scene);
    (
        [x + way[0] * along_way, y + up, z + way[2] * along_way],
        [yaw, pitch],
    )
}

/// How far the mouth is open, in degrees.
fn gape(scene: &Scene, tick: u32) -> f32 {
    let Beats { leap, bite, .. } = beats(scene);
    let open = eased(tick, leap - 2, leap + 6) as f32;
    let shut = match scene.part {
        Part::Intro => eased(tick, bite - 1, bite + 1),
        Part::Outro => eased(tick, THROWN + 4, THROWN + 8),
    };
    GAPE * open * (1.0 - shut as f32)
}

/// The lower jaw, turned around its hinge, which stays where it is.
fn jaw_shape(degrees: f32) -> DisplayTransform {
    let (sin, cos) = (degrees.to_radians() / 2.0).sin_cos();
    let [_, y, z] = HINGE.map(|axis| axis * SCALE);
    let angle = degrees.to_radians();
    let turned = [
        y * angle.cos() - z * angle.sin(),
        y * angle.sin() + z * angle.cos(),
    ];
    DisplayTransform::default()
        .uniform_scale(SCALE)
        .left_rotation([sin, 0.0, 0.0, cos])
        .translation(0.0, y - turned[0], z - turned[1])
}

/// A client turns an item display to face away from where it looks, which is where the head
/// of the model has to go.
fn facing([yaw, pitch]: [f32; 2]) -> [f32; 2] {
    [yaw + 180.0, -pitch]
}

fn shark_pose(scene: &Scene, tick: u32, shape: DisplayTransform) -> Option<Pose> {
    let Beats { leap, splash, .. } = beats(scene);
    if !(leap.saturating_sub(2)..=splash + 4).contains(&tick) {
        return None;
    }
    let (head, look) = mouth(scene, tick);
    let way = look_direction(look[0], look[1]);
    let at = std::array::from_fn(|axis| head[axis] - way[axis] * MOUTH);
    Some(Pose {
        at,
        look: facing(look),
        shape,
    })
}

fn eaten(scene: &Scene, tick: u32) -> Option<Body> {
    if !(BODY..SWALLOWED).contains(&tick) {
        return None;
    }
    let [yaw, pitch] = scene.look;
    let towards = fin_at(scene, tick).map_or(yaw, |(fin, _)| scene.aim(scene.feet(), fin)[0]);
    let stance = match tick {
        _ if tick >= INTRO.leap => Stance::Flail,
        _ if tick >= SHAKES => Stance::Shiver,
        _ => Stance::Stand,
    };
    let lifted = eased(tick, SEIZED, SWALLOWED);
    let feet = scene.feet();
    let (head, _) = mouth(scene, tick);
    Some(Body {
        at: std::array::from_fn(|axis| feet[axis] + (head[axis] - feet[axis]) * lifted),
        yaw: towards,
        pitch: if tick < INTRO.fin { pitch } else { 25.0 },
        stance,
        seat: None,
    })
}

fn thrown(scene: &Scene, tick: u32) -> Option<Body> {
    if !(THROWN..BODY_GONE).contains(&tick) {
        return None;
    }
    if tick < LANDED {
        let share = f64::from(tick - THROWN) / f64::from(LANDED - THROWN);
        let (from, _) = mouth(scene, THROWN);
        let feet = scene.feet();
        let up = hop(tick, THROWN, LANDED, 3.5);
        let mut at: [f64; 3] =
            std::array::from_fn(|axis| from[axis] + (feet[axis] - from[axis]) * share);
        at[1] += up;
        return Some(Body {
            at,
            yaw: tick as f32 * 40.0,
            pitch: ((f64::from(tick) * 0.9).sin() * 80.0) as f32,
            stance: Stance::Tumble,
            seat: None,
        });
    }
    if tick < STANDS {
        return Some(scene.standing(40.0, Stance::Cower));
    }
    Some(scene.coming_round(tick, STANDS, CALM))
}

fn splash(at: [f64; 3], count: i32) -> Burst {
    Burst {
        particle: Particle::Splash,
        at,
        spread: [2.2, 0.5, 2.2],
        speed: 0.5,
        count,
    }
}

impl Script for Shark {
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
                let watch = scene.raised(10.0, 6.0, 5.0);
                let closer = scene.raised(9.0, 3.5, 3.8);
                let wider = scene.raised(11.0, 6.0, 5.0);
                let [eye, over_head] = scene.leaving_the_eye();
                let keys = [
                    eye,
                    over_head,
                    scene.watching(30, watch, middle(1.2)),
                    scene.watching(66, closer, middle(1.4)),
                    scene.watching(84, wider, middle(3.5)),
                    scene.watching(INTRO_TICKS, scene.off_feet(3.0, 2.0, 30.0), middle(0.0)),
                ];
                along(&keys, tick)
            }
            Part::Outro => {
                let [x, y, z] = centre(scene);
                let [feet_x, _, feet_z] = scene.feet();
                let between = [(x + feet_x) / 2.0, y + 2.0, (z + feet_z) / 2.0];
                let wide = scene.raised(13.0, 7.5, 7.5);
                let turned = scene.raised(11.0, 4.0, 5.5);
                let nearing = scene.raised(6.5, 2.0, 3.4);
                let [over_head, over_eye, eye] = scene.back_in_the_eye(OVER_THE_HEAD, OUTRO_TICKS);
                let keys = [
                    scene.sky_start(),
                    scene.watching(30, wide, between),
                    scene.watching(72, turned, between),
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
            Part::Intro => vec![
                scene.off_feet(10.0, 6.0, 5.0),
                scene.off_feet(9.0, 3.5, 3.8),
                scene.off_feet(5.0, 3.0, 2.5),
            ],
            Part::Outro => vec![
                scene.off_feet(13.0, 7.5, 7.5),
                scene.off_feet(11.0, 4.0, 5.5),
                scene.off_feet(6.5, 2.0, 3.4),
                scene.off_feet(3.2, 1.0, 1.7),
            ],
        }
    }

    fn props(&self) -> &'static [Prop] {
        &PROPS
    }

    fn pose(&self, scene: &Scene, prop: usize, tick: u32) -> Option<Pose> {
        match prop {
            POOL => {
                let radius = pool_radius(scene, tick);
                let [x, y, z] = centre(scene);
                let wide = 2.0 * radius as f32;
                (radius > 0.02).then(|| Pose {
                    at: [x, y + POOL_HEIGHT, z],
                    look: [0.0, 0.0],
                    shape: DisplayTransform::default().scale(wide, 1.0, wide),
                })
            }
            FIN => fin_at(scene, tick).map(|(at, angle)| Pose {
                at,
                look: facing([angle.rem_euclid(TAU).to_degrees() as f32, 0.0]),
                shape: DisplayTransform::default().uniform_scale(SCALE),
            }),
            BODY_OF_SHARK => shark_pose(
                scene,
                tick,
                DisplayTransform::default().uniform_scale(SCALE),
            ),
            JAW => shark_pose(scene, tick, jaw_shape(gape(scene, tick))),
            _ => None,
        }
    }

    fn body(&self, scene: &Scene, tick: u32) -> Option<Body> {
        match scene.part {
            Part::Intro => eaten(scene, tick),
            Part::Outro => thrown(scene, tick),
        }
    }

    fn noise(&self, scene: &Scene, tick: u32) -> Option<Noise> {
        let Beats {
            pool_open,
            fin,
            leap,
            bite,
            splash,
            ..
        } = beats(scene);
        match (scene.part, tick) {
            (_, t) if t == pool_open => noise("entity.player.swim", 0.6),
            (_, t) if t == fin => noise("entity.fish.swim", 0.5),
            (_, t) if t == leap => noise("entity.player.splash.high_speed", 0.7),
            (Part::Intro, t) if t == bite => noise("entity.ravager.attack", 1.2),
            (Part::Intro, BURP) => noise("entity.player.burp", 0.6),
            (Part::Outro, THROWN) => noise("entity.llama.spit", 0.6),
            (Part::Outro, LANDED) => noise("entity.player.big_fall", 1.0),
            (_, t) if t == splash => noise("entity.generic.splash", 0.7),
            _ => None,
        }
    }

    fn bursts(&self, scene: &Scene, tick: u32) -> Vec<Burst> {
        let Beats {
            fin_gone,
            leap,
            splash: back,
            ..
        } = beats(scene);
        let mut bursts = Vec::new();
        if let Some((at, _)) = fin_at(scene, tick) {
            bursts.push(Burst {
                spread: [0.3, 0.1, 0.3],
                ..splash([at[0], centre(scene)[1] + 0.2, at[2]], 3)
            });
        }
        let [x, y, z] = centre(scene);
        if (fin_gone..leap).contains(&tick) {
            bursts.push(Burst {
                particle: Particle::BubblePop,
                at: [x, y + 0.2, z],
                spread: [1.2, 0.1, 1.2],
                speed: 0.05,
                count: 6,
            });
        }
        if tick == leap || tick == back {
            let (head, _) = mouth(scene, tick.max(leap + 3).min(back - 3));
            bursts.push(splash([head[0], y + 0.3, head[2]], 90));
        }
        if scene.part == Part::Outro && tick == LANDED {
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
        add_texture(pack, "shark", &palette(&COLOURS));
        add_model(pack, "shark", blocks_model("shark", &shark_blocks()));
        add_model(pack, "shark_jaw", blocks_model("shark", &jaw_blocks()));
        add_model(
            pack,
            "shark_fin",
            blocks_model("shark", &fin_blocks(8.0, 8.0)),
        );
        add_model(pack, "shark_pool", pool_model());
        add_texture(pack, "shark_pool", &pool_texture());
        pack.add(
            "assets/voidmap/textures/item/shark_pool.png.mcmeta",
            br#"{"animation":{"frametime":4}}"#.to_vec(),
        );
    }
}

const SKIN: usize = 0;
const BELLY: usize = 1;
const DARK: usize = 2;
const GUMS: usize = 3;
const TOOTH: usize = 4;
const EYE: usize = 5;
const COLOURS: [[u8; 4]; 6] = [
    [92, 112, 138, 255],
    [232, 236, 240, 255],
    [52, 66, 88, 255],
    [190, 52, 66, 255],
    [255, 255, 250, 255],
    [16, 16, 22, 255],
];

/// Its length, from the tail to the gills, as slices of how wide and how high they are.
const SLICES: [(f32, f32, f32, f32); 5] = [
    (-10.0, -5.0, 1.6, 1.8),
    (-5.0, 1.0, 3.0, 3.2),
    (1.0, 8.0, 5.0, 5.2),
    (8.0, 14.0, 6.5, 6.6),
    (14.0, 19.0, 6.5, 6.2),
];

/// A fin standing on a height, narrowing as it goes up, around this place along the body.
fn fin_blocks(at: f32, up: f32) -> Vec<Block> {
    (0..7)
        .map(|step| {
            let step = step as f32;
            let from = [7.2, up + step * 2.0, at - 6.0 + step * 1.1];
            let to = [8.8, up + step * 2.0 + 2.0, at + 5.0 - step * 0.2];
            block(from, to, SKIN)
        })
        .collect()
}

fn shark_blocks() -> Vec<Block> {
    let mut blocks = Vec::new();
    for (from, to, wide, high) in SLICES {
        let (left, right) = (8.0 - wide, 8.0 + wide);
        blocks.push(block([left, 7.0, from], [right, 8.0 + high, to], SKIN));
        blocks.push(block([left, 8.0 - high, from], [right, 7.0, to], BELLY));
    }
    // The upper jaw, with the inside of the mouth under it and a row of teeth along it.
    blocks.push(block([2.0, 7.0, 19.0], [14.0, 13.5, 25.0], SKIN));
    blocks.push(block([3.5, 7.0, 25.0], [12.5, 12.0, 30.0], SKIN));
    blocks.push(block([2.5, 6.4, 19.0], [13.5, 7.0, 29.5], GUMS));
    for step in 0..5 {
        let z = 20.0 + step as f32 * 2.0;
        blocks.push(block([2.6, 5.0, z], [3.6, 6.4, z + 1.2], TOOTH));
        blocks.push(block([12.4, 5.0, z], [13.4, 6.4, z + 1.2], TOOTH));
    }
    blocks.push(block([6.0, 5.0, 28.6], [7.2, 6.4, 29.6], TOOTH));
    blocks.push(block([8.8, 5.0, 28.6], [10.0, 6.4, 29.6], TOOTH));
    // The eyes, the gills.
    blocks.push(block([1.8, 9.6, 22.5], [14.2, 11.2, 24.0], EYE));
    for z in [14.5, 16.0, 17.5] {
        blocks.push(block([1.3, 6.0, z], [14.7, 11.0, z + 0.5], DARK));
    }
    // The fin on its back, the two on its sides, and the tail.
    blocks.extend(fin_blocks(7.0, 13.0));
    for (from, to) in [(-5.0, 1.5), (14.5, 21.0)] {
        blocks.push(block([from, 3.5, 11.0], [to, 4.5, 17.0], DARK));
    }
    for step in 0..6 {
        let step = step as f32;
        let back = -10.0 - step;
        let (top, bottom) = (8.0 + step * 2.6, 7.0 - step * 1.8);
        blocks.push(block([7.2, top, back - 1.0], [8.8, top + 4.0, back], SKIN));
        blocks.push(block(
            [7.2, bottom - 3.0, back - 1.0],
            [8.8, bottom, back],
            SKIN,
        ));
    }
    blocks
}

fn jaw_blocks() -> Vec<Block> {
    let mut blocks = vec![
        block([3.0, 2.5, 18.0], [13.0, 5.0, 25.0], BELLY),
        block([4.5, 2.5, 25.0], [11.5, 4.6, 29.0], BELLY),
        block([3.4, 5.0, 18.0], [12.6, 5.4, 28.6], GUMS),
    ];
    for step in 0..5 {
        let z = 19.5 + step as f32 * 1.9;
        blocks.push(block([3.5, 5.4, z], [4.5, 6.8, z + 1.1], TOOTH));
        blocks.push(block([11.5, 5.4, z], [12.5, 6.8, z + 1.1], TOOTH));
    }
    blocks
}

/// One flat sheet, seen from above.
fn pool_model() -> String {
    let face = r##"{"texture":"#pool","uv":[0,0,16,16]}"##;
    format!(
        r#"{{"textures":{{"particle":"voidmap:item/shark_pool","pool":"voidmap:item/shark_pool"}},"elements":[{{"from":[0,8,0],"to":[16,8,16],"shade":false,"faces":{{"up":{face},"down":{face}}}}}]}}"#
    )
}

const POOL_SIZE: u32 = 64;
const POOL_FRAMES: u32 = 4;

/// Flat blue with rings going out, a foam edge and nothing past it, frame after frame.
fn pool_texture() -> Image {
    painted(POOL_SIZE, POOL_SIZE * POOL_FRAMES, |x, y| {
        let frame = f64::from(y / POOL_SIZE) / f64::from(POOL_FRAMES);
        let middle = f64::from(POOL_SIZE) / 2.0;
        let across = (f64::from(x) + 0.5 - middle) / middle;
        let down = (f64::from(y % POOL_SIZE) + 0.5 - middle) / middle;
        let out = across.hypot(down);
        let edge = 1.0 + 0.04 * (across.atan2(down) * 9.0 + frame * TAU).sin();
        if out > edge {
            return [0, 0, 0, 0];
        }
        if out > edge - 0.1 {
            return [240, 250, 255, 255];
        }
        let ring = ((out * 4.0 - frame).rem_euclid(1.0) * 10.0) as u32;
        match ring {
            0 => [120, 196, 244, 255],
            1 | 2 => [62, 150, 226, 255],
            _ => [38, 118, 206, 255],
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::film::Film;

    const EYE_AT: [f64; 3] = [10.0, 65.62, -4.0];
    const LOOK: [f32; 2] = [170.0, 12.0];

    fn scene(part: Part) -> Scene {
        Scene::new(Film::Shark, part, EYE_AT, LOOK)
    }

    fn apart(a: [f64; 3], b: [f64; 3]) -> f64 {
        (0..3)
            .map(|axis| (a[axis] - b[axis]).powi(2))
            .sum::<f64>()
            .sqrt()
    }

    /// Its mouth shuts on the player, wherever they stand.
    #[test]
    fn the_mouth_is_on_the_player_when_it_bites() {
        let scene = scene(Part::Intro);
        let (head, _) = mouth(&scene, INTRO.bite);
        assert!(apart(head, scene.over_feet(1.0)) < 1.0);
        assert!(gape(&scene, INTRO.bite - 2) > GAPE * 0.9);
        assert_eq!(gape(&scene, INTRO.bite + 1), 0.0);
    }

    #[test]
    fn the_body_goes_with_the_mouth_shut_on_it() {
        let scene = scene(Part::Intro);
        assert!(scene.body(SWALLOWED - 1).is_some());
        assert!(scene.body(SWALLOWED).is_none());
        assert!(gape(&scene, SWALLOWED) == 0.0);
    }

    #[test]
    fn the_body_comes_out_of_the_open_mouth() {
        let scene = scene(Part::Outro);
        let body = scene.body(THROWN).unwrap();
        assert!(apart(body.at, mouth(&scene, THROWN).0) < 0.01);
        assert!(gape(&scene, THROWN) > GAPE * 0.9);
    }

    #[test]
    fn the_shark_stays_inside_the_pool_as_it_goes_through_the_ground() {
        for scene in [scene(Part::Intro), scene(Part::Outro)] {
            let Beats { leap, splash, .. } = beats(&scene);
            for tick in leap..=splash {
                let (head, _) = mouth(&scene, tick);
                if head[1] < centre(&scene)[1] + 0.5 {
                    assert!(pool_radius(&scene, tick) > 4.0, "{tick}");
                    let flat = |at: [f64; 3]| [at[0], 0.0, at[2]];
                    assert!(apart(flat(head), flat(centre(&scene))) < POOL_RADIUS);
                }
            }
        }
    }

    #[test]
    fn a_model_stays_within_what_a_client_takes() {
        for blocks in [shark_blocks(), jaw_blocks()] {
            for block in blocks {
                for axis in block.from.into_iter().chain(block.to) {
                    assert!((-16.0..=32.0).contains(&axis), "{block:?}");
                }
            }
        }
    }
}
