//! The models of the scenes, in the pack: boxes of plain colour, and flat drawings for
//! what is too small to be boxes.
//!
//! What moves by itself is an animated texture: a client plays its frames with no packet
//! at all, so that a gull beats its wings and a float dips for nothing. All the copies of
//! a texture go together, which is why a flock takes three of them, out of step.

use voidmc::ItemStack;
use voidmc_codec::Encode;
use voidmc_protocol::slot::{DataComponent, component_ids};
use voidmc_ui::{Image, ResourcePack};

use super::Model;

const ITEMS: &str = "assets/voidmap/items";
const MODELS: &str = "assets/voidmap/models/item";
const TEXTURES: &str = "assets/voidmap/textures/block";
/// The side of a drawing, in pixels: a frame of an animated texture is as high as it is wide.
const SIDE: u32 = 16;

const ALL: [Model; 11] = [
    Model::Gull,
    Model::GullOffBeat,
    Model::GullLastBeat,
    Model::Locomotive,
    Model::Carriage,
    Model::Wagon,
    Model::Angler,
    Model::Float,
    Model::Balloon,
    Model::BalloonBlue,
    Model::BalloonShadow,
];

/// What a box is painted with: a block of the game, or a drawing of the pack.
type Paint = &'static str;

const BLACK: Paint = "minecraft:block/black_concrete";
const GREY: Paint = "minecraft:block/gray_concrete";
const LIGHT: Paint = "minecraft:block/light_gray_concrete";
const WHITE: Paint = "minecraft:block/white_concrete";
const RED: Paint = "minecraft:block/red_concrete";
const YELLOW: Paint = "minecraft:block/yellow_concrete";
const BLUE: Paint = "minecraft:block/blue_concrete";
const GREEN: Paint = "minecraft:block/green_concrete";
const WOOD: Paint = "minecraft:block/spruce_planks";
const SKIN: Paint = "minecraft:block/sandstone_top";
const STRAW: Paint = "minecraft:block/hay_block_top";

/// One box of a model, from and to, in sixteenths of a block.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Part {
    from: [f32; 3],
    to: [f32; 3],
    paint: Paint,
    /// Tilted about the east-west axis through its middle, in degrees.
    tilt: f32,
}

const fn part(from: [f32; 3], to: [f32; 3], paint: Paint) -> Part {
    Part {
        from,
        to,
        paint,
        tilt: 0.0,
    }
}

/// A flat drawing lying on the ground, just over it for the two not to fight.
const fn flat(paint: Paint) -> [Part; 1] {
    [part([0.0, 0.6, 0.0], [16.0, 0.6, 16.0], paint)]
}

const GULL: [Paint; 3] = [
    "voidmap:block/life_gull_0",
    "voidmap:block/life_gull_1",
    "voidmap:block/life_gull_2",
];
const FLOAT: Paint = "voidmap:block/life_float";
const SMOKE: Paint = "voidmap:block/life_smoke";
const BALLOON_RED: Paint = "voidmap:block/life_balloon_red";
const BALLOON_BLUE: Paint = "voidmap:block/life_balloon_blue";
const BALLOON_SHADOW: Paint = "voidmap:block/life_balloon_shadow";

/// A drawing level with the middle of its model, for it to lean about itself.
const fn level(paint: Paint) -> [Part; 1] {
    [part([0.0, 8.0, 0.0], [16.0, 8.0, 16.0], paint)]
}

/// Heads of a person: a large one, as a person is seen from above.
const fn person(shirt: Paint, trousers: Paint) -> [Part; 5] {
    [
        part([5.0, 0.0, 6.5], [7.8, 6.0, 9.5], trousers),
        part([8.2, 0.0, 6.5], [11.0, 6.0, 9.5], trousers),
        part([4.5, 6.0, 6.0], [11.5, 12.0, 10.0], shirt),
        part([3.0, 7.0, 6.8], [13.0, 11.5, 9.2], shirt),
        part([5.5, 12.0, 5.5], [10.5, 16.0, 10.5], SKIN),
    ]
}

fn parts(model: Model) -> Vec<Part> {
    match model {
        Model::Gull => level(GULL[0]).to_vec(),
        Model::GullOffBeat => level(GULL[1]).to_vec(),
        Model::GullLastBeat => level(GULL[2]).to_vec(),
        Model::BalloonShadow => flat(BALLOON_SHADOW).to_vec(),
        Model::Float => flat(FLOAT).to_vec(),
        Model::Locomotive => vec![
            part([4.5, 0.5, 0.0], [11.5, 2.0, 16.0], BLACK),
            part([5.0, 2.0, 0.5], [11.0, 6.5, 10.5], GREY),
            part([4.8, 3.5, 0.3], [11.2, 4.2, 10.5], RED),
            part([4.5, 2.0, 10.5], [11.5, 7.5, 15.5], RED),
            part([4.0, 7.5, 10.0], [12.0, 8.2, 16.0], BLACK),
            part([6.8, 6.5, 2.0], [9.2, 10.0, 4.4], BLACK),
            part([6.3, 10.0, 1.5], [9.7, 10.8, 4.9], LIGHT),
            part([7.0, 6.5, 6.0], [9.0, 7.6, 8.0], YELLOW),
            part([0.0, 11.0, 0.0], [16.0, 11.0, 16.0], SMOKE),
        ],
        Model::Carriage => vec![
            part([4.5, 0.5, 0.5], [11.5, 2.0, 15.5], BLACK),
            part([4.8, 2.0, 1.0], [11.2, 7.5, 15.0], BLUE),
            part([4.6, 4.5, 2.0], [11.4, 6.2, 14.0], YELLOW),
            part([5.8, 7.5, 1.5], [10.2, 8.3, 14.5], WHITE),
            part([7.4, 8.3, 3.0], [8.6, 8.8, 13.0], LIGHT),
        ],
        Model::Wagon => vec![
            part([4.5, 0.5, 0.5], [11.5, 2.0, 15.5], BLACK),
            part([4.5, 2.0, 1.0], [5.3, 6.0, 15.0], WOOD),
            part([10.7, 2.0, 1.0], [11.5, 6.0, 15.0], WOOD),
            part([4.5, 2.0, 1.0], [11.5, 6.0, 1.8], WOOD),
            part([4.5, 2.0, 14.2], [11.5, 6.0, 15.0], WOOD),
            part([5.3, 2.0, 1.8], [10.7, 5.2, 14.2], BLACK),
        ],
        Model::Angler => {
            let mut parts = person(GREEN, BLUE).to_vec();
            parts.extend([
                // A wide straw hat: from above, it is all there is to see of them.
                part([4.5, 15.5, 5.5], [11.5, 16.0, 10.5], STRAW),
                part([5.5, 15.5, 4.5], [10.5, 16.0, 11.5], STRAW),
                part([6.0, 16.0, 6.0], [10.0, 17.5, 10.0], STRAW),
                Part {
                    from: [7.7, 8.0, -14.0],
                    to: [8.3, 8.6, 6.0],
                    paint: WOOD,
                    tilt: -22.5,
                },
                // The line, from the tip of the rod down to the water.
                part([7.9, -2.0, -12.0], [8.1, 11.5, -11.8], WHITE),
            ]);
            parts
        }
        Model::Balloon => balloon(BALLOON_RED, YELLOW),
        Model::BalloonBlue => balloon(BALLOON_BLUE, WHITE),
    }
}

/// A balloon: its envelope in rounds, each a cross of two boxes, which from above comes
/// close to a circle, on four ropes over a basket.
fn balloon(gores: Paint, crown: Paint) -> Vec<Part> {
    /// Each round from its bottom to its top, and how far from the middle it reaches.
    const ROUNDS: [[f32; 3]; 8] = [
        [4.0, 5.0, 1.4],
        [5.0, 6.0, 2.8],
        [6.0, 7.2, 4.3],
        [7.2, 8.6, 5.6],
        [8.6, 11.8, 6.6],
        [11.8, 13.6, 5.8],
        [13.6, 15.0, 4.4],
        [15.0, 16.0, 2.6],
    ];
    let cross = |[from, to, far]: [f32; 3], paint: Paint| {
        let near = far * 0.72;
        [
            part(
                [8.0 - far, from, 8.0 - near],
                [8.0 + far, to, 8.0 + near],
                paint,
            ),
            part(
                [8.0 - near, from, 8.0 - far],
                [8.0 + near, to, 8.0 + far],
                paint,
            ),
        ]
    };
    let rope = |x: f32, z: f32| part([x, 1.2, z], [x + 0.25, 4.6, z + 0.25], GREY);
    let mut parts = vec![
        part([6.9, 0.0, 6.9], [9.1, 1.4, 9.1], WOOD),
        part([6.7, 1.2, 6.7], [9.3, 1.5, 9.3], BLACK),
        rope(6.9, 6.9),
        rope(8.85, 6.9),
        rope(6.9, 8.85),
        rope(8.85, 8.85),
    ];
    for (at, round) in ROUNDS.into_iter().enumerate() {
        let paint = if at + 1 == ROUNDS.len() || at == 0 {
            crown
        } else {
            gores
        };
        parts.extend(cross(round, paint));
    }
    parts
}

impl Model {
    fn name(self) -> &'static str {
        match self {
            Self::Gull => "life_gull",
            Self::GullOffBeat => "life_gull_off",
            Self::GullLastBeat => "life_gull_last",
            Self::Locomotive => "life_locomotive",
            Self::Carriage => "life_carriage",
            Self::Wagon => "life_wagon",
            Self::Angler => "life_angler",
            Self::Float => "life_float",
            Self::Balloon => "life_balloon",
            Self::BalloonBlue => "life_balloon_blue",
            Self::BalloonShadow => "life_balloon_shadow",
        }
    }

    /// What a client without the pack gets: a flat sprite, as close as the game has.
    fn plain_item(self) -> &'static str {
        match self {
            Self::Gull | Self::GullOffBeat | Self::GullLastBeat => "minecraft:feather",
            Self::Locomotive | Self::Carriage => "minecraft:furnace_minecart",
            Self::Wagon => "minecraft:chest_minecart",
            Self::Angler | Self::Float => "minecraft:fishing_rod",
            Self::Balloon => "minecraft:red_dye",
            Self::BalloonBlue => "minecraft:blue_dye",
            Self::BalloonShadow => "minecraft:black_dye",
        }
    }

    /// The model of the pack, or the sprite for a client without it.
    pub fn item(self, has_pack: bool) -> ItemStack {
        let item = ItemStack::of(self.plain_item(), 1).expect("a vanilla item");
        if !has_pack {
            return item;
        }
        let mut body = Vec::new();
        format!("voidmap:{}", self.name()).encode(&mut body);
        item.with_component(DataComponent::Raw {
            type_id: component_ids::ITEM_MODEL,
            body,
        })
    }
}

/// The model of an item, as the client takes it.
fn model_file(parts: &[Part]) -> String {
    let mut paints: Vec<Paint> = parts.iter().map(|part| part.paint).collect();
    paints.dedup();
    paints.sort_unstable();
    paints.dedup();
    let key = |paint: Paint| paints.iter().position(|one| *one == paint).unwrap_or(0);
    let element = |part: &Part| {
        let texture = format!("#t{}", key(part.paint));
        let face = |side: &str| format!(r#""{side}":{{"texture":"{texture}"}}"#);
        let sides = ["north", "south", "east", "west", "up", "down"].map(face);
        let rotation = if part.tilt == 0.0 {
            String::new()
        } else {
            let middle = (0..3).map(|axis| (part.from[axis] + part.to[axis]) / 2.0);
            let middle: Vec<_> = middle.collect();
            format!(
                r#","rotation":{{"origin":{middle:?},"axis":"x","angle":{}}}"#,
                part.tilt
            )
        };
        format!(
            r#"{{"from":{:?},"to":{:?},"shade":false{rotation},"faces":{{{}}}}}"#,
            part.from,
            part.to,
            sides.join(",")
        )
    };
    let textures: Vec<_> = paints
        .iter()
        .enumerate()
        .map(|(index, paint)| format!(r#""t{index}":"{paint}""#))
        .collect();
    let elements: Vec<_> = parts.iter().map(element).collect();
    format!(
        r#"{{"textures":{{"particle":"{}",{}}},"elements":[{}]}}"#,
        paints[0],
        textures.join(","),
        elements.join(",")
    )
}

/// Pixels from rows of characters: a dot lets the ground through.
fn draw(image: &mut Image, top: u32, rows: &[&str], colour: impl Fn(char) -> [u8; 4]) {
    for (y, row) in rows.iter().enumerate() {
        for (x, pixel) in row.chars().enumerate() {
            if pixel != '.' {
                image.set(x as u32, top + y as u32, colour(pixel));
            }
        }
    }
}

fn gull_colour(pixel: char) -> [u8; 4] {
    match pixel {
        'k' => [40, 44, 52, 255],
        'g' => [150, 160, 172, 255],
        'o' => [240, 160, 40, 255],
        _ => [250, 251, 252, 255],
    }
}

/// A gull from above, its beak to the north, from wings spread to wings up.
const GULL_FRAMES: [[&str; 16]; 4] = [
    [
        "................",
        "................",
        "................",
        ".......oo.......",
        ".......ww.......",
        "......wwww......",
        ".kgwwwwwwwwwwgk.",
        "kggwwwwwwwwwwggk",
        ".kgwwwwwwwwwwgk.",
        "...ggwwwwwwgg...",
        ".......ww.......",
        ".......ww.......",
        "......wwww......",
        "................",
        "................",
        "................",
    ],
    [
        "................",
        "................",
        "................",
        ".......oo.......",
        ".......ww.......",
        "......wwww......",
        "...gwwwwwwwwg...",
        "..kggwwwwwwggk..",
        "..kgwwwwwwwwgk..",
        "....gwwwwwwg....",
        ".......ww.......",
        ".......ww.......",
        "......wwww......",
        "................",
        "................",
        "................",
    ],
    [
        "................",
        "................",
        "................",
        ".......oo.......",
        ".......ww.......",
        "......wwww......",
        ".....gwwwwg.....",
        "....kgwwwwgk....",
        "....kgwwwwgk....",
        ".....gwwwwg.....",
        ".......ww.......",
        ".......ww.......",
        "......wwww......",
        "................",
        "................",
        "................",
    ],
    [
        "................",
        "................",
        "................",
        ".......oo.......",
        ".......ww.......",
        "......wwww......",
        "..kgwwwwwwwwgk..",
        ".kgggwwwwwwgggk.",
        "..kggwwwwwwggk..",
        "...gwwwwwwwwg...",
        ".......ww.......",
        ".......ww.......",
        "......wwww......",
        "................",
        "................",
        "................",
    ],
];

/// The order of the frames of a beat, starting at a point of it.
fn beat(start: usize) -> String {
    let frames = [0, 3, 1, 2, 2, 1, 3, 0];
    let turned = (0..frames.len()).map(|at| frames[(at + start) % frames.len()].to_string());
    format!(
        r#"{{"animation":{{"frametime":2,"interpolate":false,"frames":[{}]}}}}"#,
        turned.collect::<Vec<_>>().join(",")
    )
}

fn gull_strip() -> Image {
    let frames = GULL_FRAMES.len() as u32;
    let mut image = Image::new(SIDE, SIDE * frames);
    for (index, frame) in GULL_FRAMES.iter().enumerate() {
        draw(&mut image, index as u32 * SIDE, frame, gull_colour);
    }
    image
}

/// The gores of a balloon, side by side: from above and from the side alike, the faces of
/// a box take the drawing by where they are in the model.
fn gores(one: [u8; 4], other: [u8; 4]) -> Image {
    let mut image = Image::new(SIDE, SIDE);
    for y in 0..SIDE {
        for x in 0..SIDE {
            image.set(x, y, if (x / 2) % 2 == 0 { one } else { other });
        }
    }
    image
}

/// A round of shade, soft at its rim, as a fine screen of dots. Half see-through pixels
/// would do on land, but they hide the real water behind them, and the bed of a lake
/// shows through as a pale patch: a pixel here is either all there or not at all.
fn balloon_shadow() -> Image {
    /// How far into the screen a dot comes, in sixteenths.
    const SCREEN: [[u32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
    const FINE_SIDE: u32 = SIDE * 4;
    let mut image = Image::new(FINE_SIDE, FINE_SIDE);
    let middle = f64::from(FINE_SIDE) / 2.0 - 0.5;
    for y in 0..FINE_SIDE {
        for x in 0..FINE_SIDE {
            let far = (f64::from(x) - middle).hypot(f64::from(y) - middle) / middle;
            let shade = 0.56 * (1.0 - far.powi(4));
            let dot = f64::from(SCREEN[y as usize % 4][x as usize % 4]) / 16.0;
            if far <= 1.0 && dot < shade {
                image.set(x, y, [10, 14, 24, 255]);
            }
        }
    }
    image
}

/// The rings around a float, out to a radius, in the strength given.
fn rings(image: &mut Image, top: u32, radius: f64, strength: u8) {
    let middle = f64::from(SIDE) / 2.0 - 0.5;
    for y in 0..SIDE {
        for x in 0..SIDE {
            let far = (f64::from(x) - middle).hypot(f64::from(y) - middle);
            if (far - radius).abs() < 0.55 {
                image.set(x, top + y, [235, 245, 255, strength]);
            }
        }
    }
}

/// How the float goes: still, nodding, and a bite that takes it under.
fn float_strip() -> Image {
    /// A ring's radius and strength, and whether the float shows.
    const FRAMES: [(f64, u8, bool); 8] = [
        (0.0, 0, true),
        (3.0, 230, true),
        (4.5, 200, true),
        (6.0, 160, true),
        (7.5, 90, true),
        (2.0, 255, false),
        (4.0, 240, false),
        (6.5, 180, false),
    ];
    let mut image = Image::new(SIDE, SIDE * FRAMES.len() as u32);
    for (index, (radius, strength, shows)) in FRAMES.iter().enumerate() {
        let top = index as u32 * SIDE;
        if *strength > 0 {
            rings(&mut image, top, *radius, *strength);
        }
        if *shows {
            for (x, y, colour) in [
                (7, 7, [230, 40, 40, 255]),
                (8, 7, [230, 40, 40, 255]),
                (7, 8, [250, 250, 250, 255]),
                (8, 8, [250, 250, 250, 255]),
            ] {
                image.set(x, top + y, colour);
            }
        }
    }
    image
}

/// Puffs leaving the chimney, at the top of the drawing, and thinning out down the train.
fn smoke_strip() -> Image {
    const FRAMES: u32 = 8;
    const PUFFS: u32 = 3;
    let mut image = Image::new(SIDE, SIDE * FRAMES);
    for frame in 0..FRAMES {
        for puff in 0..PUFFS {
            let age = (f64::from(puff) + f64::from(frame) / f64::from(FRAMES)) / f64::from(PUFFS);
            let (x, y) = (7.5 + (age * 9.0).sin() * 1.2, 3.0 + age * 12.0);
            let radius = 1.0 + age * 2.2;
            let strength = (235.0 * (1.0 - age * age)) as u8;
            for py in 0..SIDE {
                for px in 0..SIDE {
                    if (f64::from(px) - x).hypot(f64::from(py) - y) <= radius {
                        image.set(px, frame * SIDE + py, [236, 238, 242, strength]);
                    }
                }
            }
        }
    }
    image
}

const SMOKE_FRAMES: &str = r#"{"animation":{"frametime":3,"interpolate":true}}"#;

/// Long calm, a nod and its rings, again and again, and a bite now and then.
const FLOAT_FRAMES: &str = r#"{"animation":{"frametime":5,"interpolate":false,"frames":[
    {"index":0,"time":10},1,2,3,4,{"index":0,"time":8},1,2,3,4,{"index":0,"time":14},
    5,6,7,5,6,7,1,2,3,4]}}"#;

/// Adds every model of the scenes.
pub fn install(pack: &mut ResourcePack) {
    for model in ALL {
        let name = model.name();
        let item =
            format!(r#"{{"model":{{"type":"minecraft:model","model":"voidmap:item/{name}"}}}}"#);
        pack.add(format!("{ITEMS}/{name}.json"), item.into_bytes());
        pack.add(
            format!("{MODELS}/{name}.json"),
            model_file(&parts(model)).into_bytes(),
        );
    }
    for (index, start) in [0, 3, 5].into_iter().enumerate() {
        let file = format!("{TEXTURES}/life_gull_{index}.png");
        pack.add(file.clone(), gull_strip().to_png());
        pack.add(format!("{file}.mcmeta"), beat(start).into_bytes());
    }
    let textures = [
        (
            "life_balloon_red",
            gores([214, 52, 44, 255], [244, 232, 208, 255]),
        ),
        (
            "life_balloon_blue",
            gores([44, 96, 190, 255], [240, 196, 70, 255]),
        ),
        ("life_balloon_shadow", balloon_shadow()),
    ];
    for (name, image) in textures {
        pack.add(format!("{TEXTURES}/{name}.png"), image.to_png());
    }
    let file = format!("{TEXTURES}/life_float.png");
    pack.add(file.clone(), float_strip().to_png());
    pack.add(format!("{file}.mcmeta"), FLOAT_FRAMES.as_bytes().to_vec());
    let file = format!("{TEXTURES}/life_smoke.png");
    pack.add(file.clone(), smoke_strip().to_png());
    pack.add(format!("{file}.mcmeta"), SMOKE_FRAMES.as_bytes().to_vec());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_model_is_valid_json_with_its_textures_named() {
        for model in ALL {
            let file = model_file(&parts(model));
            let parsed: serde_json::Value = serde_json::from_str(&file).unwrap();
            let elements = parsed["elements"].as_array().unwrap();
            assert!(!elements.is_empty(), "{model:?}");
            for element in elements {
                let texture = element["faces"]["up"]["texture"].as_str().unwrap();
                assert!(parsed["textures"][&texture[1..]].is_string(), "{model:?}");
            }
        }
    }

    #[test]
    fn every_frame_of_an_animation_exists() {
        let meta: serde_json::Value = serde_json::from_str(&beat(3)).unwrap();
        for frame in meta["animation"]["frames"].as_array().unwrap() {
            assert!((frame.as_u64().unwrap() as usize) < GULL_FRAMES.len());
        }
        let meta: serde_json::Value = serde_json::from_str(FLOAT_FRAMES).unwrap();
        assert!(meta["animation"]["frames"].is_array());
    }
}
