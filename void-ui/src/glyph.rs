//! Pictures in text. A font maps private-use characters to images, so a text
//! block can draw a board or a map that the client still treats as text: every
//! glyph can carry its own click and hover.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::pack::ResourcePack;
use crate::text::Text;
use voidmc::TextColor;

/// Height of a text line: a taller picture needs one glyph per line.
const LINE_HEIGHT: u32 = 9;
/// Distance from the top of a line to its baseline.
const BASELINE: i32 = 7;
/// Where the client opens a tooltip relative to the mouse.
const TOOLTIP_OFFSET: i32 = 12;
const FIRST_GLYPH: u32 = 0xE000;
const LAST_GLYPH: u32 = 0xF8FF;

/// An RGBA picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    width: u32,
    height: u32,
    pixels: Vec<[u8; 4]>,
}

impl Image {
    pub fn new(width: u32, height: u32) -> Self {
        Self::filled(width, height, [0; 4])
    }

    pub fn filled(width: u32, height: u32, color: [u8; 4]) -> Self {
        Self {
            width,
            height,
            pixels: vec![color; (width * height) as usize],
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        self.pixels[(y * self.width + x) as usize]
    }

    pub fn set(&mut self, x: u32, y: u32, color: [u8; 4]) {
        self.pixels[(y * self.width + x) as usize] = color;
    }

    pub fn to_png(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .expect("writing to a Vec cannot fail");
        writer
            .write_image_data(self.pixels.as_flattened())
            .expect("the buffer matches the header");
        writer.finish().expect("writing to a Vec cannot fail");
        bytes
    }

    /// Fonts trim fully clear columns from a glyph's width. A nearly invisible
    /// pixel at each end keeps it, so advances can be computed from the image.
    fn pin_width(&mut self) {
        for x in [0, self.width - 1] {
            if self.pixel(x, 0)[3] == 0 {
                self.set(x, 0, [255, 255, 255, 1]);
            }
        }
    }
}

/// A picture cut into text lines. Tiles of the same size line up in a grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    lines: Vec<String>,
}

/// A picture that sits on the mouse: put it in [`Text::on_hover`]. The client
/// moves tooltips with the mouse by itself, so the server need not know where it
/// is. Needs [`ResourcePack::hide_tooltip_frames`] to show without a box.
#[derive(Debug, Clone, PartialEq)]
pub struct Cursor(Text);

impl Cursor {
    pub fn text(&self) -> Text {
        self.0.clone()
    }
}

/// Collects pictures into one font and hands back what to draw them with.
#[derive(Debug)]
pub struct FontBuilder {
    namespace: String,
    name: String,
    next: u32,
    providers: Vec<Value>,
    images: Vec<(String, Image)>,
    advances: BTreeMap<i32, char>,
}

impl FontBuilder {
    pub fn new(namespace: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            name: name.into(),
            next: FIRST_GLYPH,
            providers: Vec::new(),
            images: Vec::new(),
            advances: BTreeMap::new(),
        }
    }

    /// The id to pass to [`Text::font`] and [`glyph_grid`].
    pub fn id(&self) -> String {
        format!("{}:{}", self.namespace, self.name)
    }

    fn glyph(&mut self) -> char {
        assert!(self.next <= LAST_GLYPH, "out of private-use characters");
        let glyph = char::from_u32(self.next).expect("private-use characters are valid");
        self.next += 1;
        glyph
    }

    /// A blank character that moves the pen by `pixels`, which may be negative.
    fn advance(&mut self, pixels: i32) -> char {
        if let Some(glyph) = self.advances.get(&pixels) {
            return *glyph;
        }
        let glyph = self.glyph();
        self.advances.insert(pixels, glyph);
        glyph
    }

    fn add_image(&mut self, image: Image) -> String {
        let name = format!("{}_{}", self.name, self.images.len());
        self.images.push((name.clone(), image));
        format!("{}:font/{name}.png", self.namespace)
    }

    /// Adds a picture to lay out in a grid. Its height must be a multiple of 9.
    pub fn tile(&mut self, image: Image) -> Tile {
        assert!(
            image.height > 0 && image.height.is_multiple_of(LINE_HEIGHT),
            "a tile is a whole number of {LINE_HEIGHT}px lines"
        );
        let mut image = image;
        image.pin_width();
        let gap = self.advance(-1);
        let glyphs: Vec<char> = (0..image.height / LINE_HEIGHT)
            .map(|_| self.glyph())
            .collect();
        let file = self.add_image(image);
        self.providers.push(json!({
            "type": "bitmap",
            "file": file,
            "height": LINE_HEIGHT,
            "ascent": BASELINE,
            "chars": glyphs.iter().map(char::to_string).collect::<Vec<_>>(),
        }));
        Tile {
            lines: glyphs.iter().map(|glyph| format!("{glyph}{gap}")).collect(),
        }
    }

    /// Adds a picture centred on the mouse.
    pub fn cursor(&mut self, image: Image) -> Cursor {
        let (width, height) = (image.width as i32, image.height as i32);
        let mut image = image;
        image.pin_width();
        let before = -(TOOLTIP_OFFSET + width / 2);
        // A glyph advances one pixel past its width. Ending on zero keeps the
        // tooltip empty, so the client never shifts it away from a screen edge.
        let glyph_advance = width + 1;
        let (start, end) = (self.advance(before), self.advance(-before - glyph_advance));
        let glyph = self.glyph();
        let file = self.add_image(image);
        self.providers.push(json!({
            "type": "bitmap",
            "file": file,
            "height": height,
            "ascent": height / 2 + BASELINE - TOOLTIP_OFFSET,
            "chars": [glyph.to_string()],
        }));
        Cursor(
            Text::new(format!("{start}{glyph}{end}"))
                .font(self.id())
                .color(TextColor::White)
                .no_shadow(),
        )
    }

    /// Writes the font and its images into the pack.
    pub fn install(self, pack: &mut ResourcePack) {
        let mut providers = self.providers;
        let advances: serde_json::Map<String, Value> = self
            .advances
            .iter()
            .map(|(pixels, glyph)| (glyph.to_string(), json!(pixels)))
            .collect();
        providers.push(json!({ "type": "space", "advances": advances }));
        let font = json!({ "providers": providers });
        pack.add(
            format!("assets/{}/font/{}.json", self.namespace, self.name),
            font.to_string().into_bytes(),
        );
        for (name, image) in self.images {
            pack.add(
                format!("assets/{}/textures/font/{name}.png", self.namespace),
                image.to_png(),
            );
        }
    }
}

/// One cell of a [`glyph_grid`].
#[derive(Debug, Clone, PartialEq)]
pub struct Cell<'a> {
    tile: &'a Tile,
    click: Option<String>,
    hover: Option<Text>,
}

impl<'a> Cell<'a> {
    pub fn new(tile: &'a Tile) -> Self {
        Self {
            tile,
            click: None,
            hover: None,
        }
    }

    pub fn on_click(mut self, id: impl Into<String>) -> Self {
        self.click = Some(id.into());
        self
    }

    pub fn on_hover(mut self, text: Text) -> Self {
        self.hover = Some(text);
        self
    }

    fn line(&self, line: usize) -> Text {
        let mut text = Text::new(self.tile.lines[line].clone());
        if let Some(id) = &self.click {
            text = text.on_click(id.clone());
        }
        if let Some(hover) = &self.hover {
            text = text.on_hover(hover.clone());
        }
        text
    }
}

/// Draws a grid of same-height tiles as one text block for [`crate::Dialog::body`].
pub fn glyph_grid<'a>(
    font: &str,
    columns: u32,
    rows: u32,
    cell: impl Fn(u32, u32) -> Cell<'a>,
) -> Text {
    let mut lines = Vec::new();
    for row in 0..rows {
        let cells: Vec<Cell> = (0..columns).map(|column| cell(column, row)).collect();
        let height = cells.first().map_or(0, |cell| cell.tile.lines.len());
        for line in 0..height {
            lines.extend(cells.iter().map(|cell| cell.line(line)));
            lines.push(Text::new("\n"));
        }
    }
    lines.pop();
    Text::new("")
        .font(font)
        .color(TextColor::White)
        .no_shadow()
        .extend(lines)
}

#[cfg(test)]
mod tests {
    use ussr_nbt::owned::{List, Tag};

    use super::*;
    use crate::text::tests::tag;

    fn font() -> FontBuilder {
        FontBuilder::new("test", "ui")
    }

    #[test]
    fn a_tile_has_one_glyph_per_text_line() {
        let tile = font().tile(Image::new(18, 18));
        assert_eq!(tile.lines.len(), 2);
    }

    #[test]
    #[should_panic(expected = "whole number")]
    fn a_tile_must_fit_whole_lines() {
        font().tile(Image::new(18, 10));
    }

    #[test]
    fn tiles_share_the_gap_but_not_their_glyphs() {
        let mut font = font();
        let (a, b) = (font.tile(Image::new(9, 9)), font.tile(Image::new(9, 9)));
        assert_ne!(a.lines[0], b.lines[0]);
        assert_eq!(a.lines[0].chars().nth(1), b.lines[0].chars().nth(1));
    }

    #[test]
    fn a_cursor_cancels_the_tooltip_offset_and_ends_on_zero() {
        let mut font = font();
        font.cursor(Image::new(18, 18));
        assert!(font.advances.contains_key(&-21));
        assert!(font.advances.contains_key(&2));
        assert_eq!(font.providers[0]["ascent"], 4);
    }

    #[test]
    fn clear_pictures_keep_their_width() {
        let mut image = Image::new(4, 9);
        image.pin_width();
        assert_ne!(image.pixel(0, 0)[3], 0);
        assert_ne!(image.pixel(3, 0)[3], 0);
    }

    #[test]
    fn a_grid_lays_out_each_line_of_each_row() {
        let tile = font().tile(Image::new(18, 18));
        let text = glyph_grid("test:ui", 3, 2, |_, _| Cell::new(&tile).on_click("a:b"));
        let compound = text.to_compound();
        let Some(Tag::List(List::Compound(children))) = tag(&compound, "extra") else {
            panic!("no children");
        };
        let clickable = children.iter().filter(|c| tag(c, "click_event").is_some());
        assert_eq!(clickable.count(), 3 * 2 * 2);
        assert_eq!(children.len(), 3 * 2 * 2 + 3);
    }

    #[test]
    fn installing_writes_the_font_and_every_image() {
        let mut font = font();
        font.tile(Image::new(9, 9));
        font.cursor(Image::new(9, 9));
        let mut pack = ResourcePack::new("test");
        font.install(&mut pack);
        assert!(pack.contains("assets/test/font/ui.json"));
        assert!(pack.contains("assets/test/textures/font/ui_0.png"));
        assert!(pack.contains("assets/test/textures/font/ui_1.png"));
    }

    #[test]
    fn images_encode_as_png() {
        assert!(Image::new(2, 2).to_png().starts_with(b"\x89PNG"));
    }
}
