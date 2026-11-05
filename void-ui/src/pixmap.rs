//! Pictures that change at runtime, painted without a new pack. Each cell is a
//! white swatch tinted by its text color, so the pack only holds one swatch per
//! row, and a whole picture is one line of text whose pen moves over the cells.

use voidmc::TextColor;

use crate::glyph::{FontBuilder, Image};
use crate::text::Text;

/// Height of a text line, and the baseline inside it, as drawn by the client.
const LINE_HEIGHT: i32 = 9;
const BASELINE: i32 = 7;
/// Pen moves are sums of powers of two, up to this one.
const LARGEST_STEP: i32 = 512;
/// A glyph advances one pixel past its width.
const GLYPH_GAP: i32 = 1;
const OUTLINE: [u8; 3] = [0, 0, 0];

/// A mark drawn over the picture, with its own click and hover.
#[derive(Debug, Clone, PartialEq)]
pub struct Stamp {
    col: u32,
    row: u32,
    color: [u8; 3],
    click: Option<String>,
    hover: Option<Text>,
}

impl Stamp {
    /// A square with a black outline, centred on a cell.
    pub fn new(col: u32, row: u32, color: [u8; 3]) -> Self {
        Self {
            col,
            row,
            color,
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
}

/// The glyphs to paint pictures of up to `rows` rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pixmap {
    font: String,
    cell: i32,
    stamp: i32,
    cells: Vec<char>,
    outlines: Vec<char>,
    fills: Vec<char>,
    /// Largest move first.
    steps: Vec<(i32, char)>,
}

impl Pixmap {
    /// Adds the glyphs to `font`: three swatches per row, and the pen moves.
    /// `cell` and `stamp` are sizes in GUI pixels.
    pub fn new(font: &mut FontBuilder, cell: u32, rows: u32, stamp: u32) -> Self {
        assert!(cell > 0, "a cell needs a size");
        assert!(stamp >= cell && stamp >= 3, "a stamp covers its cell");
        let overhang = (stamp - cell) as i32 / 2;
        let mut cells = Vec::new();
        let mut outlines = Vec::new();
        let mut fills = Vec::new();
        for row in 0..rows as i32 {
            // The client refuses an ascent above the height, so the first row
            // sits as high as that allows.
            let ascent = cell as i32 * (1 - row);
            cells.push(font.swatch(cell, cell, ascent));
            outlines.push(font.swatch(stamp, stamp, ascent + overhang));
            fills.push(font.swatch(stamp - 2, stamp - 2, ascent + overhang - 1));
        }
        let mut steps = Vec::new();
        let mut size = LARGEST_STEP;
        while size > 0 {
            steps.push((size, font.advance(size)));
            steps.push((-size, font.advance(-size)));
            size /= 2;
        }
        Self {
            font: font.id(),
            cell: cell as i32,
            stamp: stamp as i32,
            cells,
            outlines,
            fills,
            steps,
        }
    }

    /// Text lines the picture covers, which the widget must have to be clickable.
    pub fn lines(&self) -> usize {
        let bottom = BASELINE - self.cell + self.cell * self.cells.len() as i32;
        (bottom + LINE_HEIGHT - 1) as usize / LINE_HEIGHT as usize
    }

    fn text(&self, children: Vec<Text>) -> Text {
        Text::new("")
            .font(self.font.clone())
            .color(TextColor::White)
            .no_shadow()
            .extend(children)
    }

    fn moves(&self, mut pixels: i32) -> String {
        let mut moves = String::new();
        for (size, glyph) in &self.steps {
            while size.signum() == pixels.signum() && size.abs() <= pixels.abs() {
                moves.push(*glyph);
                pixels -= size;
            }
        }
        moves
    }

    /// Paints `image` on one line of text, one cell per pixel. Pixels with no
    /// alpha stay empty. Cells are drawn in square zones of `zone` cells a side,
    /// and `click` gives the id of a zone from its middle cell. `stamps` go on
    /// top, and a stamp wins the clicks over the cells under it.
    pub fn paint(
        &self,
        image: &Image,
        zone: u32,
        click: impl Fn(u32, u32) -> Option<String>,
        stamps: &[Stamp],
    ) -> Text {
        assert!(image.height() as usize <= self.cells.len(), "too many rows");
        assert!(zone > 0, "a zone needs a size");
        let mut pen = Pen::new(self);
        for top in (0..image.height()).step_by(zone as usize) {
            for left in (0..image.width()).step_by(zone as usize) {
                pen.move_to(left as i32 * self.cell);
                pen.flush();
                let (right, bottom) = (
                    (left + zone).min(image.width()),
                    (top + zone).min(image.height()),
                );
                for row in top..bottom {
                    pen.move_to(left as i32 * self.cell);
                    for col in left..right {
                        pen.cell(image, col, row);
                    }
                }
                let runs = pen.take();
                let middle = (left + (right - left) / 2, top + (bottom - top) / 2);
                pen.group(runs, click(middle.0, middle.1), None);
            }
        }
        for stamp in stamps {
            pen.stamp(stamp);
        }
        pen.move_to(image.width() as i32 * self.cell);
        pen.flush();
        self.text(pen.parts)
    }

    /// Puts a column of text to the right of a painted picture `width` cells
    /// wide. Lines are centred in the column: dialog text is centred, and this
    /// needs no font metrics. The picture's own line stays empty, so `lines`
    /// start on the second one.
    pub fn beside(
        &self,
        picture: Text,
        width: u32,
        gap: u32,
        column: u32,
        lines: Vec<Text>,
    ) -> Text {
        let left = width as i32 * self.cell + gap as i32;
        let spacer = || Text::new(self.moves(left));
        let fill = self.moves(gap as i32 + column as i32);
        let mut block = vec![picture, self.text(vec![Text::new(fill)])];
        let blank = self.lines().saturating_sub(lines.len() + 1);
        for line in lines
            .into_iter()
            .chain(std::iter::repeat_n(Text::new(""), blank))
        {
            block.push(Text::new("\n"));
            block.push(self.text(vec![spacer()]));
            block.push(line);
        }
        Text::new("").extend(block)
    }
}

/// Writes glyphs in order, tracking where the next one lands, and merges
/// neighbours of the same color into one text.
struct Pen<'a> {
    pixmap: &'a Pixmap,
    x: i32,
    color: Option<[u8; 3]>,
    content: String,
    runs: Vec<Text>,
    parts: Vec<Text>,
}

impl<'a> Pen<'a> {
    fn new(pixmap: &'a Pixmap) -> Self {
        Self {
            pixmap,
            x: 0,
            color: None,
            content: String::new(),
            runs: Vec::new(),
            parts: Vec::new(),
        }
    }

    fn move_to(&mut self, x: i32) {
        self.content.push_str(&self.pixmap.moves(x - self.x));
        self.x = x;
    }

    fn end_run(&mut self) {
        if self.content.is_empty() {
            return;
        }
        let mut run = Text::new(std::mem::take(&mut self.content));
        if let Some([red, green, blue]) = self.color {
            run = run.color(TextColor::rgb(
                u32::from(red) << 16 | u32::from(green) << 8 | u32::from(blue),
            ));
        }
        self.runs.push(run);
    }

    /// Draws a glyph `width` pixels wide, leaving the pen `advance` past it.
    fn glyph(&mut self, glyph: char, color: [u8; 3], width: i32, advance: i32) {
        if self.color != Some(color) {
            self.end_run();
            self.color = Some(color);
        }
        self.content.push(glyph);
        self.x += width + GLYPH_GAP;
        self.move_to(self.x - width - GLYPH_GAP + advance);
    }

    fn cell(&mut self, image: &Image, col: u32, row: u32) {
        let cell = self.pixmap.cell;
        let [red, green, blue, alpha] = image.pixel(col, row);
        if alpha == 0 {
            self.move_to(self.x + cell);
        } else {
            let glyph = self.pixmap.cells[row as usize];
            self.glyph(glyph, [red, green, blue], cell, cell);
        }
    }

    fn take(&mut self) -> Vec<Text> {
        self.end_run();
        std::mem::take(&mut self.runs)
    }

    /// Ends what is pending, outside any zone or stamp.
    fn flush(&mut self) {
        self.end_run();
        self.parts.append(&mut self.runs);
    }

    fn group(&mut self, runs: Vec<Text>, click: Option<String>, hover: Option<Text>) {
        let mut group = Text::new("").extend(runs);
        if let Some(id) = click {
            group = group.on_click(id);
        }
        if let Some(text) = hover {
            group = group.on_hover(text);
        }
        self.parts.push(group);
    }

    fn stamp(&mut self, stamp: &Stamp) {
        let Pixmap {
            cell,
            stamp: size,
            outlines,
            fills,
            ..
        } = self.pixmap;
        let Some((&outline, &fill)) = outlines
            .get(stamp.row as usize)
            .zip(fills.get(stamp.row as usize))
        else {
            return;
        };
        let left = stamp.col as i32 * cell - (size - cell) / 2;
        self.move_to(left);
        self.flush();
        self.glyph(outline, OUTLINE, *size, 1);
        self.glyph(fill, stamp.color, size - 2, size - 2);
        let runs = self.take();
        self.group(runs, stamp.click.clone(), stamp.hover.clone());
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use ussr_nbt::owned::{Compound, List, Tag};

    use super::*;
    use crate::text::tests::{tag, text_of};

    const RED: [u8; 4] = [255, 0, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];

    fn pixmap(rows: u32) -> Pixmap {
        Pixmap::new(&mut FontBuilder::new("test", "map"), 2, rows, 7)
    }

    fn children(text: &Compound) -> &[Compound] {
        match tag(text, "extra") {
            Some(Tag::List(List::Compound(children))) => children,
            _ => &[],
        }
    }

    /// What the client would do with a text: follows the pen and records glyphs.
    #[derive(Default)]
    struct Replay {
        x: i32,
        lines: usize,
        /// `(x, glyph) -> (color, click id)`, later glyphs on top.
        drawn: HashMap<(i32, char), (String, Option<String>)>,
        order: Vec<char>,
        end_of_line: Vec<i32>,
    }

    impl Replay {
        fn run(pixmap: &Pixmap, text: &Text) -> Self {
            let mut replay = Self::default();
            replay.walk(pixmap, &text.to_compound(), "", None);
            replay.end_of_line.push(replay.x);
            replay
        }

        fn advance(pixmap: &Pixmap, glyph: char) -> Option<(i32, bool)> {
            let find = |glyphs: &[char]| glyphs.contains(&glyph);
            if let Some((size, _)) = pixmap.steps.iter().find(|(_, step)| *step == glyph) {
                Some((*size, false))
            } else if find(&pixmap.cells) {
                Some((pixmap.cell + GLYPH_GAP, true))
            } else if find(&pixmap.outlines) {
                Some((pixmap.stamp + GLYPH_GAP, true))
            } else if find(&pixmap.fills) {
                Some((pixmap.stamp - 2 + GLYPH_GAP, true))
            } else {
                None
            }
        }

        fn walk(&mut self, pixmap: &Pixmap, text: &Compound, color: &str, click: Option<String>) {
            let own = tag(text, "color").map(|color| text_of(Some(color)));
            let color = own.as_deref().unwrap_or(color);
            let click = match tag(text, "click_event") {
                Some(Tag::Compound(event)) => Some(text_of(tag(event, "id"))),
                _ => click,
            };
            for glyph in text_of(tag(text, "text")).chars() {
                if glyph == '\n' {
                    self.end_of_line.push(self.x);
                    self.lines += 1;
                    self.x = 0;
                    continue;
                }
                let Some((advance, visible)) = Self::advance(pixmap, glyph) else {
                    continue;
                };
                if visible {
                    self.drawn
                        .insert((self.x, glyph), (color.to_string(), click.clone()));
                    self.order.push(glyph);
                }
                self.x += advance;
            }
            for child in children(text) {
                self.walk(pixmap, child, color, click.clone());
            }
        }
    }

    fn checker() -> Image {
        let mut image = Image::new(6, 4);
        for row in 0..4 {
            for col in 0..6 {
                image.set(col, row, if col < 3 { RED } else { BLUE });
            }
        }
        image
    }

    #[test]
    fn each_pixel_lands_on_its_cell_with_its_color() {
        let pixmap = pixmap(4);
        let replay = Replay::run(&pixmap, &pixmap.paint(&checker(), 2, |_, _| None, &[]));
        assert_eq!(replay.drawn.len(), 24);
        for row in 0..4 {
            for col in 0..6 {
                let (color, _) = &replay.drawn[&(col * 2, pixmap.cells[row as usize])];
                let wanted = if col < 3 { "#ff0000" } else { "#0000ff" };
                assert_eq!(color, wanted);
            }
        }
    }

    #[test]
    fn the_pen_ends_at_the_right_edge() {
        let pixmap = pixmap(4);
        let replay = Replay::run(&pixmap, &pixmap.paint(&checker(), 2, |_, _| None, &[]));
        assert_eq!(replay.x, 12);
    }

    #[test]
    fn neighbours_of_one_color_share_a_text() {
        let pixmap = pixmap(4);
        let compound = pixmap.paint(&checker(), 8, |_, _| None, &[]).to_compound();
        let colored: usize = children(&compound)
            .iter()
            .flat_map(children)
            .filter(|run| tag(run, "color").is_some())
            .count();
        assert_eq!(colored, 8);
    }

    #[test]
    fn a_clear_pixel_is_skipped_without_shifting_the_row() {
        let mut image = checker();
        image.set(1, 2, [9, 9, 9, 0]);
        let pixmap = pixmap(4);
        let replay = Replay::run(&pixmap, &pixmap.paint(&image, 8, |_, _| None, &[]));
        assert_eq!(replay.drawn.len(), 23);
        assert!(!replay.drawn.contains_key(&(2, pixmap.cells[2])));
        assert!(replay.drawn.contains_key(&(4, pixmap.cells[2])));
    }

    #[test]
    fn zones_report_their_middle_cell_on_every_cell() {
        let pixmap = pixmap(4);
        let text = pixmap.paint(&checker(), 2, |c, r| Some(format!("a:{c}/{r}")), &[]);
        let replay = Replay::run(&pixmap, &text);
        let click = |col: i32, row: usize| replay.drawn[&(col * 2, pixmap.cells[row])].1.clone();
        assert_eq!(click(0, 0).as_deref(), Some("a:1/1"));
        assert_eq!(click(1, 1).as_deref(), Some("a:1/1"));
        assert_eq!(click(5, 3).as_deref(), Some("a:5/3"));
    }

    #[test]
    fn a_partial_zone_at_the_edge_still_has_a_middle_inside_the_picture() {
        let pixmap = pixmap(4);
        let wide = Image::filled(5, 3, RED);
        let text = pixmap.paint(&wide, 4, |c, r| Some(format!("a:{c}/{r}")), &[]);
        let replay = Replay::run(&pixmap, &text);
        assert_eq!(
            replay.drawn[&(8, pixmap.cells[2])].1.as_deref(),
            Some("a:4/1")
        );
    }

    #[test]
    fn stamps_come_after_the_cells_and_keep_their_events() {
        let pixmap = pixmap(4);
        let stamp = Stamp::new(2, 1, [1, 2, 3])
            .on_click("a:stamp")
            .on_hover(Text::new("tip"));
        let text = pixmap.paint(&checker(), 2, |_, _| Some("a:zone".into()), &[stamp]);
        let replay = Replay::run(&pixmap, &text);
        assert_eq!(replay.order.len(), 26);
        assert_eq!(replay.order[24], pixmap.outlines[1]);
        assert_eq!(replay.order[25], pixmap.fills[1]);
        // Centred on the cell at x=4, with the fill one pixel in.
        let (color, click) = &replay.drawn[&(3, pixmap.fills[1])];
        assert_eq!(color, "#010203");
        assert_eq!(click.as_deref(), Some("a:stamp"));
        assert!(replay.drawn.contains_key(&(2, pixmap.outlines[1])));
        assert_eq!(replay.x, 12);

        let compound = text.to_compound();
        let group = children(&compound)
            .iter()
            .find(|part| tag(part, "hover_event").is_some());
        assert!(group.is_some());
    }

    #[test]
    fn a_stamp_below_the_last_row_is_dropped() {
        let pixmap = pixmap(4);
        let stamps = [Stamp::new(0, 9, [1, 2, 3])];
        let replay = Replay::run(&pixmap, &pixmap.paint(&checker(), 2, |_, _| None, &stamps));
        assert_eq!(replay.order.len(), 24);
    }

    #[test]
    fn every_ascent_is_within_what_the_client_accepts() {
        let mut font = FontBuilder::new("test", "map");
        Pixmap::new(&mut font, 2, 72, 7);
        let mut pack = crate::ResourcePack::new("test");
        font.install(&mut pack);
        let built = pack.build();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(built.bytes())).unwrap();
        let file = archive.by_name("assets/test/font/map.json").unwrap();
        let font: serde_json::Value = serde_json::from_reader(file).unwrap();
        let bitmaps = font["providers"].as_array().unwrap().iter();
        let bitmaps: Vec<_> = bitmaps.filter(|p| p["type"] == "bitmap").collect();
        assert_eq!(bitmaps.len(), 72 * 3);
        for bitmap in bitmaps {
            assert!(bitmap["ascent"].as_i64() <= bitmap["height"].as_i64());
        }
    }

    #[test]
    fn the_picture_covers_whole_text_lines() {
        // 5px above the first row, then 72 rows of 2px.
        assert_eq!(pixmap(72).lines(), 17);
        assert_eq!(pixmap(2).lines(), 1);
    }

    #[test]
    fn long_moves_use_several_steps() {
        let pixmap = pixmap(1);
        assert_eq!(pixmap.moves(0), "");
        assert_eq!(pixmap.moves(3).chars().count(), 2);
        assert_eq!(pixmap.moves(-1024).chars().count(), 2);
    }

    #[test]
    fn lines_beside_share_one_width_and_fill_the_picture_height() {
        let pixmap = pixmap(40);
        let image = Image::filled(6, 40, RED);
        let picture = pixmap.paint(&image, 2, |_, _| None, &[]);
        let text = pixmap.beside(picture, 6, 8, 100, vec![Text::new("a"), Text::new("b")]);
        let replay = Replay::run(&pixmap, &text);
        assert_eq!(replay.lines + 1, pixmap.lines());
        assert_eq!(replay.end_of_line[0], 12 + 8 + 100);
        assert!(replay.end_of_line[1..].iter().all(|x| *x == 12 + 8));
    }

    #[test]
    fn more_lines_than_the_picture_is_tall_are_all_kept() {
        let pixmap = pixmap(2);
        let picture = pixmap.paint(&Image::filled(1, 2, RED), 2, |_, _| None, &[]);
        let lines = vec![Text::new("a"); 5];
        let replay = Replay::run(&pixmap, &pixmap.beside(picture, 1, 0, 10, lines));
        assert_eq!(replay.lines, 5);
    }
}
