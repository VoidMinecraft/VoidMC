//! The pictures of a scene and what they need of the pack. It restyles what every screen of
//! the client shares, as long as the pack is loaded: no blur behind a screen, plain buttons
//! that let the world through, and nothing of what a dialog shows that a scene has no use for.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use voidmc_ui::{Image, ResourcePack};

use crate::font::{self, NAMESPACE};
use crate::props::Prop;

pub(crate) const ART_FONT: &str = "voidnpc:art";
const ART_FONT_FILE: &str = "assets/voidnpc/font/art.json";
const FIRST_GLYPH: u32 = 0xE100;
const LAST_GLYPH: u32 = 0xF8FF;

const BLUR_FILE: &str = "assets/minecraft/post_effect/blur.json";
const NO_BLUR: &str = r#"{"targets":{},"passes":[]}"#;
const SHADE_FILE: &str = "assets/minecraft/textures/gui/inworld_menu_background.png";
const SPRITES: &str = "assets/minecraft/textures/gui/sprites";
const BUTTON_SIZE: (u32, u32) = (200, 20);
const BUTTON_SLICES: &str =
    r#"{"gui":{"scaling":{"type":"nine_slice","width":200,"height":20,"border":3}}}"#;
const BUTTON_FILL: [u8; 4] = [10, 14, 18, 190];
const BUTTON_EDGE: [u8; 4] = [255, 255, 255, 34];
const HOVERED_FILL: [u8; 4] = [255, 255, 255, 58];
const HOVERED_EDGE: [u8; 4] = [255, 255, 255, 210];

/// A client keeps glyphs on a sheet of this size and has no room for a larger one.
const MOST_GLYPH: u32 = 256;
/// Bars and veil are solid, so a small picture drawn large does: this one is wider than any
/// screen once blown up.
const BAR_SCALE: u32 = 8;
const BAR_WIDTH: u32 = MOST_GLYPH;
/// What lies over the world at the top of a scene, and at the bottom, over this scale.
const TOP_BAR: u32 = 3;
const BOTTOM_BAR: u32 = 8;
/// How far down the action bar its bar starts.
pub const BOTTOM_BAR_AT: i32 = 56;
/// A title is drawn four times its size, and this covers any screen.
const VEIL_SIZE: (u32, u32) = (64, 40);
const VEIL_SCALE: u32 = 10;
/// How far over the middle of the screen a title starts, in its own pixels.
const TITLE_TOP: i32 = 10;
/// A dialog title starts this far from the top of the screen.
pub const DIALOG_TITLE_TOP: i32 = 13;
const LINE_BASELINE: i32 = 7;
const RULE_HALF: u32 = 62;
const RULE_GAP: u32 = 7;
const CROSS: u32 = 7;
const CARD_THICKNESS: f64 = 0.25;

/// A picture to draw on a [`crate::Canvas`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Picture {
    glyph: char,
    width: i32,
}

impl Picture {
    pub fn glyph(&self) -> char {
        self.glyph
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    /// A glyph moves the pen one pixel past its width.
    pub(crate) fn advance(&self) -> i32 {
        self.width + 1
    }
}

/// A texture of the game on a [`crate::Canvas`]. The client trims its clear sides, so its
/// width is only about right: see [`crate::Canvas::icon`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Icon {
    glyph: char,
    width: i32,
}

impl Icon {
    pub fn glyph(&self) -> char {
        self.glyph
    }

    pub(crate) fn advance(&self) -> i32 {
        self.width + 1
    }
}

/// Collects the pictures of all NPCs into the font of the pack.
#[derive(Debug)]
pub struct ArtBuilder {
    next: u32,
    providers: Vec<Value>,
    images: Vec<(String, Image)>,
    icons: BTreeMap<(String, i32), Icon>,
    cards: Vec<Card>,
}

#[derive(Debug)]
struct Card {
    front: Image,
    back: Image,
    width: f64,
    height: f64,
}

impl Card {
    /// A thin box around the middle of the item, its front to the north: the client turns
    /// an item display half a turn, which shows that side first.
    fn model(&self, name: &str) -> Value {
        let (left, right) = (8.0 - self.width / 2.0, 8.0 + self.width / 2.0);
        let (bottom, top) = (8.0 - self.height / 2.0, 8.0 + self.height / 2.0);
        let whole = json!([0, 0, 16, 16]);
        let edge = json!([0, 0, 1, 1]);
        json!({
            "textures": {
                "particle": format!("{NAMESPACE}:item/{name}_back"),
                "front": format!("{NAMESPACE}:item/{name}_front"),
                "back": format!("{NAMESPACE}:item/{name}_back"),
            },
            "elements": [{
                "from": [left, bottom, 8.0 - CARD_THICKNESS / 2.0],
                "to": [right, top, 8.0 + CARD_THICKNESS / 2.0],
                "faces": {
                    "north": { "uv": whole, "texture": "#front" },
                    "south": { "uv": whole, "texture": "#back" },
                    "east": { "uv": edge, "texture": "#back" },
                    "west": { "uv": edge, "texture": "#back" },
                    "up": { "uv": edge, "texture": "#back" },
                    "down": { "uv": edge, "texture": "#back" },
                },
            }],
        })
    }
}

impl Default for ArtBuilder {
    fn default() -> Self {
        Self {
            next: FIRST_GLYPH,
            providers: vec![font::move_provider()],
            images: Vec::new(),
            icons: BTreeMap::new(),
            cards: Vec::new(),
        }
    }
}

impl ArtBuilder {
    fn glyph(&mut self) -> char {
        assert!(self.next <= LAST_GLYPH, "out of private-use characters");
        let glyph = char::from_u32(self.next).expect("private-use characters are valid");
        self.next += 1;
        glyph
    }

    /// Adds a picture whose top sits `y` pixels down the line. White takes the colour it
    /// is drawn with. It keeps its width, however clear its sides are.
    pub fn picture(&mut self, image: Image, y: i32) -> Picture {
        self.scaled(image, y, 1)
    }

    /// Adds a picture drawn `scale` times its size, for one past what a glyph can hold.
    pub fn scaled(&mut self, image: Image, y: i32, scale: u32) -> Picture {
        let (width, height) = (image.width(), image.height());
        assert!(
            width <= MOST_GLYPH && height <= MOST_GLYPH,
            "a glyph holds {MOST_GLYPH} pixels at most: draw it smaller, and larger with `scaled`"
        );
        let glyph = self.glyph();
        let mut image = image;
        for x in [0, width - 1] {
            if image.pixel(x, 0)[3] == 0 {
                image.set(x, 0, [255, 255, 255, 1]);
            }
        }
        let name = format!("art_{}", self.images.len());
        self.providers.push(json!({
            "type": "bitmap",
            "file": format!("{NAMESPACE}:font/{name}.png"),
            "height": height * scale,
            "ascent": LINE_BASELINE - y,
            "chars": [glyph.to_string()],
        }));
        self.images.push((name, image));
        Picture {
            glyph,
            width: (width * scale) as i32,
        }
    }

    /// A texture of the game, such as `item/emerald`, drawn `size` pixels high with its top
    /// `y` pixels down the line. One call per place: the place is part of the glyph.
    pub fn icon(&mut self, texture: &str, size: u32, y: i32) -> Icon {
        let key = (format!("{texture}@{size}"), y);
        if let Some(icon) = self.icons.get(&key) {
            return *icon;
        }
        let glyph = self.glyph();
        self.providers.push(json!({
            "type": "bitmap",
            "file": format!("minecraft:{texture}.png"),
            "height": size,
            "ascent": LINE_BASELINE - y,
            "chars": [glyph.to_string()],
        }));
        let icon = Icon {
            glyph,
            width: size as i32,
        };
        self.icons.insert(key, icon);
        icon
    }

    /// A flat thing with two faces to show in the world, `width` and `height` being in
    /// sixteenths of a block at the scale of one.
    pub fn card(&mut self, front: Image, back: Image, width: f64, height: f64) -> Prop {
        let model = format!("{NAMESPACE}:card_{}", self.cards.len());
        self.cards.push(Card {
            front,
            back,
            width,
            height,
        });
        Prop { model }
    }

    fn install(self, pack: &mut ResourcePack) {
        for (index, card) in self.cards.iter().enumerate() {
            let name = format!("card_{index}");
            let textures = format!("assets/{NAMESPACE}/textures/item/{name}");
            pack.add(format!("{textures}_front.png"), card.front.to_png());
            pack.add(format!("{textures}_back.png"), card.back.to_png());
            pack.add(
                format!("assets/{NAMESPACE}/models/item/{name}.json"),
                card.model(&name).to_string().into_bytes(),
            );
            let item = json!({
                "model": { "type": "minecraft:model", "model": format!("{NAMESPACE}:item/{name}") },
            });
            pack.add(
                format!("assets/{NAMESPACE}/items/{name}.json"),
                item.to_string().into_bytes(),
            );
        }
        let font = json!({ "providers": self.providers });
        pack.add(ART_FONT_FILE, font.to_string().into_bytes());
        for (name, image) in self.images {
            pack.add(
                format!("assets/{NAMESPACE}/textures/font/{name}.png"),
                image.to_png(),
            );
        }
    }
}

/// A plain button with its outermost pixels left out, so a cell one pixel wide shows nothing.
fn button(fill: [u8; 4], edge: [u8; 4]) -> Image {
    let (width, height) = BUTTON_SIZE;
    let mut image = Image::new(width, height);
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let outline = x == 1 || y == 1 || x == width - 2 || y == height - 2;
            image.set(x, y, if outline { edge } else { fill });
        }
    }
    image
}

fn rule() -> Image {
    let width = RULE_HALF * 2 + RULE_GAP * 2 + CROSS;
    let mut image = Image::new(width, CROSS);
    let middle = CROSS / 2;
    for x in 0..width {
        let from_side = x.min(width - 1 - x);
        let on_line = x < RULE_HALF || x >= width - RULE_HALF;
        if on_line {
            let fade = (from_side * 255 / RULE_HALF.max(1)).min(255) as u8;
            image.set(x, middle, [255, 255, 255, fade]);
        }
    }
    let start = RULE_HALF + RULE_GAP;
    for step in 0..CROSS {
        for thick in 0..3 {
            image.set(start + step, middle - 1 + thick, [255; 4]);
            image.set(start + middle - 1 + thick, step, [255; 4]);
        }
    }
    image
}

/// Glass that gets clearer towards its left side.
pub fn glass(width: u32, height: u32, alpha: u8) -> Image {
    let mut image = Image::new(width, height);
    for x in 0..width {
        let fade = (x * 16).min(width) * u32::from(alpha) / width;
        for y in 0..height {
            image.set(x, y, [255, 255, 255, fade as u8]);
        }
    }
    image
}

/// What every scene draws with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Art {
    pub(crate) top_bar: Picture,
    pub(crate) bottom_bar: Picture,
    pub(crate) veil: Picture,
    pub(crate) rule: Picture,
    pub(crate) pin: Picture,
    pub(crate) mark: Picture,
    pub(crate) fill: Picture,
}

impl Art {
    /// Adds what scenes share to what the interfaces of the NPCs added, and writes it all
    /// into the pack.
    pub(crate) fn new(mut builder: ArtBuilder, pack: &mut ResourcePack) -> Self {
        let bar = |height: u32| Image::filled(BAR_WIDTH, height, [0, 0, 0, 255]);
        let top_bar = builder.scaled(bar(TOP_BAR), -DIALOG_TITLE_TOP, BAR_SCALE);
        let bottom_bar = builder.scaled(bar(BOTTOM_BAR), BOTTOM_BAR_AT, BAR_SCALE);
        let (width, height) = VEIL_SIZE;
        let veil_image = Image::filled(width, height, [0, 0, 0, 255]);
        let veil_y = TITLE_TOP - (height * VEIL_SCALE) as i32 / 2;
        let veil = builder.scaled(veil_image, veil_y, VEIL_SCALE);
        let rule = builder.picture(rule(), crate::view::RULE_Y);
        let pin = builder.picture(Image::filled(1, 9, [255, 255, 255, 3]), 0);
        let mut cross = Image::new(CROSS, CROSS);
        for step in 0..CROSS {
            for thick in 0..3 {
                cross.set(step, CROSS / 2 - 1 + thick, [255; 4]);
                cross.set(CROSS / 2 - 1 + thick, step, [255; 4]);
            }
        }
        let mark = builder.picture(cross, 1);
        let fill = builder.picture(
            Image::filled(
                crate::view::CHOICE_WIDTH as u32 - 2,
                18,
                [255, 255, 255, 62],
            ),
            -5,
        );
        font::install(pack);
        install_screens(pack);
        builder.install(pack);
        Self {
            top_bar,
            bottom_bar,
            veil,
            rule,
            pin,
            mark,
            fill,
        }
    }
}

fn install_screens(pack: &mut ResourcePack) {
    pack.add(BLUR_FILE, NO_BLUR.as_bytes().to_vec());
    pack.add(SHADE_FILE, Image::new(16, 16).to_png());
    for (name, sprite) in [
        ("widget/button", button(BUTTON_FILL, BUTTON_EDGE)),
        (
            "widget/button_highlighted",
            button(HOVERED_FILL, HOVERED_EDGE),
        ),
    ] {
        pack.add(format!("{SPRITES}/{name}.png"), sprite.to_png());
        pack.add(
            format!("{SPRITES}/{name}.png.mcmeta"),
            BUTTON_SLICES.as_bytes().to_vec(),
        );
    }
    let blank = |width, height| Image::new(width, height).to_png();
    for name in [
        "dialog/warning_button",
        "dialog/warning_button_disabled",
        "dialog/warning_button_highlighted",
    ] {
        pack.add(format!("{SPRITES}/{name}.png"), blank(20, 20));
    }
    for name in ["widget/scroller", "widget/scroller_background"] {
        pack.add(format!("{SPRITES}/{name}.png"), blank(6, 32));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_icon_at_the_same_place_is_one_glyph() {
        let mut art = ArtBuilder::default();
        let one = art.icon("item/emerald", 16, 10);
        assert_eq!(art.icon("item/emerald", 16, 10), one);
        assert_ne!(art.icon("item/emerald", 16, 20), one);
    }

    #[test]
    fn a_picture_keeps_its_width_with_clear_sides() {
        let mut art = ArtBuilder::default();
        let picture = art.picture(Image::new(10, 9), 0);
        assert_eq!(picture.advance(), 11);
        assert_ne!(art.images[0].1.pixel(0, 0)[3], 0);
        assert_ne!(art.images[0].1.pixel(9, 0)[3], 0);
    }

    #[test]
    #[should_panic(expected = "at most")]
    fn a_picture_too_large_for_a_glyph_is_refused() {
        ArtBuilder::default().picture(Image::new(300, 9), 0);
    }

    #[test]
    fn a_scaled_picture_counts_its_drawn_width() {
        let picture = ArtBuilder::default().scaled(Image::new(10, 2), 0, 4);
        assert_eq!(picture.width(), 40);
    }

    #[test]
    fn the_pack_gets_rows_art_and_a_clear_background() {
        let mut pack = ResourcePack::new("test");
        Art::new(ArtBuilder::default(), &mut pack);
        for file in [
            ART_FONT_FILE,
            BLUR_FILE,
            SHADE_FILE,
            "assets/voidnpc/font/row_0.json",
        ] {
            assert!(pack.contains(file), "{file}");
        }
    }

    #[test]
    fn a_button_shows_nothing_at_its_very_edge() {
        let image = button(BUTTON_FILL, BUTTON_EDGE);
        assert_eq!(image.pixel(0, 0)[3], 0);
    }
}
