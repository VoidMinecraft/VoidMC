//! The info panel: drawn glyphs in one line of a text display, stuck to its marker.
//!
//! Glyphs can draw anything and move the pen, so one line of text places the panel and
//! its text to the pixel. The panels and shifted copies of the client's letters join
//! the default font: fonts of the same name add up across packs, and the client's
//! letters stay. A glyph cannot rise above its line, so everything hangs below it.

use voidmc_ui::{Image, ResourcePack};

use crate::alphabet::LETTERS;
use crate::cursor;
use crate::markers::MarkerKind;
use crate::noise::fractal;
use crate::settings;
use crate::transition;
use crate::wheel::CATEGORIES;

/// How wide the detail panel and the bubble's name are.
const WIDTH: u32 = 150;
const SHEET_WIDTH: u32 = 176;
const SHEET_HEIGHT: u32 = 70;
/// Where the header of a sheet ends.
const SHEET_HEADER: u32 = 30;
const SHEET_ROWS: [i32; 5] = [7, 17, 35, 46, 57];
const SHEET_HEADER_TEXT: i32 = 27;
const SHEET_BODY_TEXT: i32 = 8;
const BARE_ROWS: [i32; 5] = [0, 10, 22, 33, 44];
const BUBBLE_SIZE: u32 = 34;
const NAME_TAG_HEIGHT: u32 = 13;
const NAME_TAG_TOP: i32 = 36;
const NAME_TAG_TEXT: i32 = 39;
const BUBBLE_DETAIL_TOP: i32 = 52;
const FLOURISH_WIDTH: u32 = 120;
const FLOURISH_HEIGHT: u32 = 7;
const FLOURISH_TOP: i32 = 18;
const TITLE_ROWS: [i32; 4] = [27, 39, 50, 61];
const DETAIL_HEIGHT: u32 = 38;
const DETAIL_INDENT: i32 = 8;
const DETAIL_LINES: [i32; 3] = [6, 17, 28];
/// Under the tallest mark of a region's sign.
const SIGN_ROW: i32 = 35;

/// A line of text has its top this far above where the bitmap of a letter sits.
const LINE_ASCENT: i32 = 7;
/// Accented letters come in taller cells, whose top stands higher.
const ACCENT_ASCENT: i32 = 10;
const ACCENT_HEIGHT: i32 = 12;
const LETTER_HEIGHT: i32 = 8;
/// What a space takes in the default font.
const SPACE_ADVANCE: i32 = 4;

const FIRST_SHEET: u32 = 0xE100;
const FIRST_BUBBLE: u32 = 0xE108;
const NAME_TAG: char = '\u{E110}';
const DETAIL: char = '\u{E111}';
const FLOURISH: char = '\u{E112}';
const FIRST_MARK: u32 = 0xE118;
const FIRST_VEIL: u32 = 0xE120;
const FIRST_WHEEL: u32 = 0xE130;
const FIRST_FORWARD: u32 = 0xE200;
const FIRST_BACK: u32 = 0xE220;
const SHIFT_BITS: u32 = 10;
const FIRST_LETTERS: u32 = 0xE400;
const LETTERS_PER_ROW: u32 = 0x100;
const FONT: &str = "assets/minecraft/font/default.json";
/// The skin of what sees for the eye on a flight. A spectator sees through invisibility, so
/// for the ticks before the view moves onto it, the mannequin would show around the eye
/// as a ghost. With nothing on its skin, there is nothing to show.
pub const EYE_SKIN: &str = "voidmap:entity/nobody";
const EYE_SKIN_FILE: &str = "assets/voidmap/textures/entity/nobody.png";
const SKIN_SIZE: u32 = 64;

/// The client's glyph atlas is 256 wide, so that the veil comes in tiles. The title that
/// shows it draws four times larger: this covers a screen at any gui scale over 1.
const VEIL_TILE: (u32, u32) = (240, 203);
const VEIL_COLUMNS: u32 = 3;
const VEIL_ROWS: u32 = 2;
const VEIL_SEED: u32 = 0x5EED;
const VEIL_LIGHT: [f64; 3] = [247.0, 249.0, 252.0];
const VEIL_SHADE: [f64; 3] = [176.0, 188.0, 208.0];

const WHEEL_SIZE: u32 = 222;
const WHEEL_INNER: f64 = 57.0;
const WHEEL_OUTER: f64 = 110.0;
/// The half width of the gap between two sectors, in pixels.
const WHEEL_GAP: f64 = 1.5;
/// How far over its line the dial goes, for its text to fall on rows that exist already:
/// there is little room left for new ones.
const WHEEL_TOP: i32 = -30;
const WHEEL_COUNT_ROW: i32 = 27;
/// The choices sit above the crosshair, which stays in the middle of the wheel.
const WHEEL_BEFORE_ROW: i32 = 46;
const WHEEL_CHOICE_ROW: i32 = 57;
const WHEEL_AFTER_ROW: i32 = 69;
const WHEEL_DONE_ROW: i32 = 96;
const WHEEL_DONE: &str = "Relâcher : valider";
/// Where each sector's label goes: off the middle of the wheel, and on what row.
const WHEEL_LABELS_AT: [(i32, i32); CATEGORIES] =
    [(0, 0), (79, 50), (49, 144), (-49, 144), (-79, 50)];
/// How far off the middle of the wheel the arrows stand, and how large they are.
const WHEEL_ARROW_UP: f64 = 43.0;
const WHEEL_ARROW_DOWN: f64 = 10.0;
const WHEEL_ARROW_SIDE: f64 = 40.0;
const WHEEL_ARROW_SIZE: f64 = 5.0;
const WHEEL_ON: [u8; 4] = [226, 176, 72, 235];
const WHEEL_OFF: [u8; 4] = [22, 26, 36, 215];
const WHEEL_MIDDLE: [u8; 4] = [10, 12, 18, 235];

const FILL: [u8; 4] = [14, 17, 24, 225];
const EDGE: [u8; 4] = [4, 5, 8, 255];

/// A font sheet of the client, as its default font lays it out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sheet {
    Ascii,
    Accented,
    Latin,
}

const SHEETS: [Sheet; 3] = [Sheet::Ascii, Sheet::Accented, Sheet::Latin];

impl Sheet {
    fn file(self) -> &'static str {
        match self {
            Self::Ascii => "minecraft:font/ascii.png",
            Self::Accented => "minecraft:font/accented.png",
            Self::Latin => "minecraft:font/nonlatin_european.png",
        }
    }

    fn rows(self) -> usize {
        match self {
            Self::Ascii => 16,
            Self::Accented => 75,
            Self::Latin => 67,
        }
    }

    fn metrics(self) -> (i32, i32) {
        match self {
            Self::Accented => (ACCENT_HEIGHT, ACCENT_ASCENT),
            Self::Ascii | Self::Latin => (LETTER_HEIGHT, LINE_ASCENT),
        }
    }
}

/// A letter of the default font.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Letter {
    pub glyph: char,
    pub sheet: Sheet,
    pub row: u8,
    pub column: u8,
    pub advance: u8,
}

fn letter(glyph: char) -> Option<usize> {
    LETTERS.iter().position(|letter| letter.glyph == glyph)
}

/// Without the `§` codes, which take no room.
fn visible(text: &str) -> impl Iterator<Item = char> + '_ {
    let mut code = false;
    text.chars().filter(move |glyph| {
        let shown = !code && *glyph != '§';
        code = !code && *glyph == '§';
        shown
    })
}

/// How far a text moves the pen, in the client's default font.
pub fn text_width(text: &str) -> i32 {
    visible(text)
        .map(|glyph| match letter(glyph) {
            Some(index) => i32::from(LETTERS[index].advance),
            None => SPACE_ADVANCE,
        })
        .sum()
}

/// Cuts a text at its spaces into lines that fit.
fn wrapped(text: &str, width: i32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split(' ') {
        match lines.last_mut() {
            Some(line) if text_width(line) + SPACE_ADVANCE + text_width(word) <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

/// The look of the panel, which a player picks with `/map style`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    /// A full sheet with a coloured header, which always says everything.
    #[default]
    Sheet,
    /// Text alone, with nothing behind it.
    Bare,
    /// A round bubble with the icon, and the name under it.
    Bubble,
    /// A large title over a flourish, as on a fantasy map.
    Title,
}

const STYLES: [Style; 4] = [Style::Sheet, Style::Bare, Style::Bubble, Style::Title];

impl Style {
    /// Text with nothing behind it needs a shadow to stand out from the ground.
    pub fn has_background(self) -> bool {
        matches!(self, Self::Sheet | Self::Bubble)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Sheet => "Fiche",
            Self::Bare => "Texte seul",
            Self::Bubble => "Bulle",
            Self::Title => "Grand titre",
        }
    }

    pub fn next(self) -> Self {
        STYLES[(self.index() + 1) % STYLES.len()]
    }

    pub const ALL: [Self; 4] = STYLES;

    fn index(self) -> usize {
        STYLES.iter().position(|style| *style == self).unwrap_or(0)
    }
}

/// What a marker of the map can be drawn with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// A small drop pointing at the ground.
    Pin,
    /// An arrow to lay on the ground.
    Arrow,
    /// A ring to lay on the ground.
    Ring,
    /// A disc for a head to sit on.
    Disc,
    Lock,
    Unknown,
    /// A small triangle pointing down, for a head to show where it stands.
    Tip,
}

const MARKS: [Mark; 7] = [
    Mark::Pin,
    Mark::Arrow,
    Mark::Ring,
    Mark::Disc,
    Mark::Lock,
    Mark::Unknown,
    Mark::Tip,
];

const PIN_COLOUR: [u8; 4] = [236, 92, 84, 255];
const SIGN_COLOUR: [u8; 4] = [238, 232, 214, 255];
const QUESTION: [&str; 16] = [
    "    ########    ",
    "  ############  ",
    " ####      #### ",
    " ###        ### ",
    "            ### ",
    "           #### ",
    "         #####  ",
    "       #####    ",
    "      ####      ",
    "      ###       ",
    "      ###       ",
    "                ",
    "                ",
    "      ###       ",
    "      ###       ",
    "      ###       ",
];
const LOCK: [&str; 16] = [
    "     ######     ",
    "    ##    ##    ",
    "   ##      ##   ",
    "   ##      ##   ",
    "   ##      ##   ",
    "   ##      ##   ",
    " ############## ",
    " ############## ",
    " ######  ###### ",
    " #####    ##### ",
    " ######  ###### ",
    " ######  ###### ",
    " ############## ",
    " ############## ",
    " ############## ",
    "                ",
];

impl Mark {
    fn index(self) -> usize {
        MARKS.iter().position(|mark| *mark == self).unwrap_or(0)
    }

    fn glyph(self) -> char {
        char::from_u32(FIRST_MARK + self.index() as u32).unwrap_or(' ')
    }

    /// The text that draws it, with a client that has the pack or without.
    pub fn text(self, drawn: bool) -> String {
        match (drawn, self) {
            (true, _) => format!("§f{}", self.glyph()),
            (false, Self::Pin) => "§c▼".into(),
            (false, Self::Arrow) => "§c▲".into(),
            (false, Self::Ring) => "§c○".into(),
            (false, Self::Disc) => "§f●".into(),
            (false, Self::Lock) => "§7X".into(),
            (false, Self::Unknown) => "§7?".into(),
            (false, Self::Tip) => "§c▼".into(),
        }
    }

    /// Its height, in pixels of text.
    pub fn height(self) -> u32 {
        self.image().height()
    }

    fn image(self) -> Image {
        match self {
            Self::Pin => drop_shape(),
            Self::Arrow => arrow(),
            Self::Ring => ring(),
            Self::Disc => disc(24, FILL, EDGE),
            Self::Lock => bitmap_of(&LOCK, SIGN_COLOUR, 2),
            Self::Unknown => bitmap_of(&QUESTION, SIGN_COLOUR, 2),
            Self::Tip => tip(),
        }
    }
}

fn tip() -> Image {
    let (width, height) = (11_u32, 7_u32);
    let mut image = Image::new(width, height);
    for y in 0..height {
        for x in y.min(width / 2)..width - y.min(width / 2) {
            let edge = y == 0 || x <= y || x + 1 + y >= width;
            image.set(x, y, if edge { EDGE } else { PIN_COLOUR });
        }
    }
    image
}

fn drop_shape() -> Image {
    let (width, height) = (11_i32, 15_i32);
    let mut image = Image::new(width as u32, height as u32);
    let (centre, radius) = (5.0_f64, 5.5_f64);
    for y in 0..height {
        for x in 0..width {
            let dx = f64::from(x) - centre;
            let inside = if y <= 5 {
                dx.hypot(f64::from(y) - centre) <= radius
            } else {
                dx.abs() <= radius * f64::from(height - 1 - y) / f64::from(height - 6)
            };
            if inside {
                let hole = dx.hypot(f64::from(y) - centre) <= 1.8;
                let color = if hole {
                    [255, 255, 255, 255]
                } else {
                    PIN_COLOUR
                };
                image.set(x as u32, y as u32, color);
            }
        }
    }
    image
}

fn arrow() -> Image {
    let size = 17_i32;
    let mut image = Image::new(size as u32, size as u32);
    for y in 0..size {
        for x in 0..size {
            let half = f64::from(y) / 2.0;
            let notch = (f64::from(y) - 12.0).max(0.0) * 2.0;
            let dx = (f64::from(x) - 8.0).abs();
            if dx <= half && dx >= notch {
                image.set(x as u32, y as u32, PIN_COLOUR);
            }
        }
    }
    image
}

fn ring() -> Image {
    let size = 25;
    let mut image = Image::new(size, size);
    for y in 0..size {
        for x in 0..size {
            let from = (f64::from(x) - 12.0).hypot(f64::from(y) - 12.0);
            if (9.5..=12.0).contains(&from) {
                image.set(x, y, PIN_COLOUR);
            }
        }
    }
    image
}

fn disc(size: u32, fill: [u8; 4], edge: [u8; 4]) -> Image {
    let mut image = Image::new(size, size);
    let centre = f64::from(size - 1) / 2.0;
    for y in 0..size {
        for x in 0..size {
            let from = (f64::from(x) - centre).hypot(f64::from(y) - centre);
            if from <= centre + 0.4 {
                let color = if from > centre - 1.2 { edge } else { fill };
                image.set(x, y, color);
            }
        }
    }
    image
}

/// An ascii drawing, each character standing for a square of `pixels`.
fn bitmap_of(rows: &[&str; 16], color: [u8; 4], pixels: u32) -> Image {
    let mut image = Image::new(16 * pixels, 16 * pixels);
    for (row, line) in rows.iter().enumerate() {
        for (column, pixel) in line.bytes().enumerate() {
            if pixel == b'#' {
                for dy in 0..pixels {
                    for dx in 0..pixels {
                        image.set(column as u32 * pixels + dx, row as u32 * pixels + dy, color);
                    }
                }
            }
        }
    }
    image
}

/// Every row of text the styles write on, from the top of the line.
fn text_rows() -> Vec<i32> {
    let bubble = DETAIL_LINES.map(|line| BUBBLE_DETAIL_TOP + line);
    let mut rows: Vec<i32> = SHEET_ROWS
        .iter()
        .chain(&BARE_ROWS)
        .chain(&TITLE_ROWS)
        .chain(&bubble)
        .chain(&[NAME_TAG_TEXT, SIGN_ROW])
        .chain(&[
            WHEEL_COUNT_ROW,
            WHEEL_BEFORE_ROW,
            WHEEL_CHOICE_ROW,
            WHEEL_AFTER_ROW,
            WHEEL_DONE_ROW,
        ])
        .chain(WHEEL_LABELS_AT.iter().map(|(_, row)| row))
        .copied()
        .collect();
    rows.sort_unstable();
    rows.dedup();
    rows
}

type Rgb = [u8; 3];

/// What a card is about: this sets its colour and icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subject {
    Place(MarkerKind),
    You,
}

const SUBJECTS: [Subject; 5] = [
    Subject::Place(MarkerKind::Village),
    Subject::Place(MarkerKind::Ruins),
    Subject::Place(MarkerKind::Mine),
    Subject::Place(MarkerKind::Lighthouse),
    Subject::You,
];

impl Subject {
    fn accent(self) -> Rgb {
        match self {
            Self::Place(MarkerKind::Village) => [86, 200, 120],
            Self::Place(MarkerKind::Ruins) => [205, 184, 140],
            Self::Place(MarkerKind::Mine) => [226, 140, 62],
            Self::Place(MarkerKind::Lighthouse) => [244, 205, 84],
            Self::You => [92, 170, 255],
        }
    }

    /// Text colour closest to the accent.
    fn title_code(self) -> char {
        match self {
            Self::Place(MarkerKind::Village) => 'a',
            Self::Place(MarkerKind::Ruins) => 'f',
            Self::Place(MarkerKind::Mine) => '6',
            Self::Place(MarkerKind::Lighthouse) => 'e',
            Self::You => 'b',
        }
    }

    fn icon(self) -> [&'static str; 16] {
        match self {
            Self::Place(MarkerKind::Village) => [
                "                ",
                "       ##       ",
                "      ####      ",
                "     ######     ",
                "    ########    ",
                "   ##########   ",
                "  ############  ",
                " ############## ",
                "   oooooooooo   ",
                "   oooooooooo   ",
                "   ooo    ooo   ",
                "   ooo ## ooo   ",
                "   ooo ## ooo   ",
                "   ooo ## ooo   ",
                "   oooo##oooo   ",
                "                ",
            ],
            Self::Place(MarkerKind::Ruins) => [
                "                ",
                "   ####         ",
                "  ########  ##  ",
                "  ###  ##   ##  ",
                "  ##        ##  ",
                "  ##        ##  ",
                "  ##    #   ##  ",
                "  ##        ##  ",
                "  ##        #   ",
                "  ##            ",
                "  ##       ##   ",
                "  ##  #    ##   ",
                " ####     ####  ",
                " ####  #  ####  ",
                "################",
                "                ",
            ],
            Self::Place(MarkerKind::Mine) => [
                "                ",
                "    #######     ",
                "  ##########    ",
                " ###    #####   ",
                " #       ####   ",
                "        ######  ",
                "       ooo####  ",
                "      ooo  ###  ",
                "     ooo    ### ",
                "    ooo     ### ",
                "   ooo       ## ",
                "  ooo        ## ",
                " ooo          # ",
                " oo           # ",
                "                ",
                "                ",
            ],
            Self::Place(MarkerKind::Lighthouse) => [
                "       ##       ",
                "##    ####    ##",
                "  ##  ####  ##  ",
                "     oooooo     ",
                "      oooo      ",
                "      ####      ",
                "      ####      ",
                "      oooo      ",
                "      oooo      ",
                "     ######     ",
                "     ######     ",
                "     oooooo     ",
                "     oooooo     ",
                "    ########    ",
                "   ##########   ",
                "                ",
            ],
            Self::You => [
                "      ####      ",
                "    ########    ",
                "   ##########   ",
                "  ####oooo####  ",
                "  ###oooooo###  ",
                "  ###oooooo###  ",
                "  ###oooooo###  ",
                "  ####oooo####  ",
                "   ##########   ",
                "    ########    ",
                "     ######     ",
                "      ####      ",
                "      ####      ",
                "       ##       ",
                "       ##       ",
                "                ",
            ],
        }
    }

    fn index(self) -> usize {
        SUBJECTS
            .iter()
            .position(|subject| *subject == self)
            .unwrap_or(0)
    }
}

fn blend(from: Rgb, to: [u8; 4], share: f64) -> [u8; 4] {
    let mix = |a: u8, b: u8| (f64::from(a) * share + f64::from(b) * (1.0 - share)) as u8;
    [
        mix(from[0], to[0]),
        mix(from[1], to[1]),
        mix(from[2], to[2]),
        to[3],
    ]
}

fn opaque(rgb: Rgb) -> [u8; 4] {
    [rgb[0], rgb[1], rgb[2], 255]
}

fn draw_icon(image: &mut Image, subject: Subject, left: u32, top: u32) {
    let accent = subject.accent();
    for (row, line) in subject.icon().iter().enumerate() {
        for (column, pixel) in line.bytes().enumerate() {
            let color = match pixel {
                b'#' => opaque(accent),
                b'o' => [236, 238, 242, 255],
                _ => continue,
            };
            image.set(left + column as u32, top + row as u32, color);
        }
    }
}

/// The sheet: a dark panel with a header in the colour of its subject.
fn sheet(subject: Subject) -> Image {
    let accent = subject.accent();
    let mut image = Image::new(SHEET_WIDTH, SHEET_HEIGHT);
    let (right, bottom) = (SHEET_WIDTH - 1, SHEET_HEIGHT - 1);
    for y in 0..SHEET_HEIGHT {
        for x in 0..SHEET_WIDTH {
            let edge = x.min(right - x).min(y.min(bottom - y));
            let corner = x.min(right - x) + y.min(bottom - y);
            let color = match (corner, edge) {
                (0 | 1, _) => continue,
                (2, _) | (_, 0) => EDGE,
                (3, _) | (_, 1) => blend(accent, [0, 0, 0, 255], 0.9),
                _ if y == SHEET_HEADER => blend(accent, FILL, 0.55),
                _ if y < SHEET_HEADER => {
                    let fade = 0.34 - 0.2 * f64::from(y) / f64::from(SHEET_HEADER);
                    blend(accent, FILL, fade)
                }
                _ => FILL,
            };
            image.set(x, y, color);
        }
    }
    draw_icon(&mut image, subject, 6, 6);
    image
}

fn bubble(subject: Subject) -> Image {
    let mut image = disc(BUBBLE_SIZE, FILL, opaque(subject.accent()));
    let inset = (BUBBLE_SIZE - 16) / 2;
    draw_icon(&mut image, subject, inset, inset);
    image
}

/// A rounded strip, for a line of text.
fn rounded(width: u32, height: u32) -> Image {
    let mut image = Image::new(width, height);
    let (right, bottom) = (width - 1, height - 1);
    for y in 0..height {
        for x in 0..width {
            let edge = x.min(right - x).min(y.min(bottom - y));
            let corner = x.min(right - x) + y.min(bottom - y);
            match (corner, edge) {
                (0 | 1, _) => {}
                (2, _) | (_, 0) => image.set(x, y, EDGE),
                _ => image.set(x, y, FILL),
            }
        }
    }
    image
}

/// A thin line that swells into a diamond, under a title.
fn flourish() -> Image {
    let mut image = Image::new(FLOURISH_WIDTH, FLOURISH_HEIGHT);
    let middle = FLOURISH_HEIGHT / 2;
    let centre = FLOURISH_WIDTH as i32 / 2;
    for x in 0..FLOURISH_WIDTH {
        let from = (x as i32 - centre).abs();
        let fade = 255 - (from * 255 / centre).min(235);
        image.set(x, middle, [214, 190, 130, fade as u8]);
        let swell = middle as i32 - from;
        for y in 0..FLOURISH_HEIGHT as i32 {
            if (y - middle as i32).abs() <= swell {
                image.set(x, y as u32, [232, 206, 140, 255]);
            }
        }
    }
    image
}

/// What the panel says about the place under the look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub subject: Subject,
    pub title: String,
    pub kind: String,
    /// What and where, in a line.
    pub kicker: String,
    /// One short line.
    pub info: String,
    pub status: String,
    pub description: String,
    /// What a click does, for the styles that say it.
    pub action: String,
    /// The place is selected, so the panel shows all of it.
    pub open: bool,
}

fn veil_glyph(column: u32, row: u32) -> char {
    char::from_u32(FIRST_VEIL + row * VEIL_COLUMNS + column).expect("a private use character")
}

/// The line that fills the screen with cloud, as a title.
pub fn veil() -> String {
    let (width, _) = VEIL_TILE;
    let mut pen = Pen::default();
    for row in 0..VEIL_ROWS {
        for column in 0..VEIL_COLUMNS {
            pen.go_to((column * width) as i32);
            pen.draw(veil_glyph(column, row), width as i32 + 1);
        }
    }
    pen.go_to((VEIL_COLUMNS * width) as i32);
    pen.title
}

fn wheel_glyph(sector: usize) -> char {
    char::from_u32(FIRST_WHEEL + sector as u32).expect("a private use character")
}

/// Which sector a point of the dial falls in, clockwise with the first on top, unless
/// it falls in a gap between two.
fn wheel_sector(dx: f64, dy: f64) -> Option<usize> {
    let span = std::f64::consts::TAU / CATEGORIES as f64;
    let angle = (dx.atan2(-dy) + span / 2.0).rem_euclid(std::f64::consts::TAU);
    let sector = (angle / span) as usize;
    let off_edge = (angle - sector as f64 * span).min((sector + 1) as f64 * span - angle);
    (off_edge * dx.hypot(dy) > WHEEL_GAP).then_some(sector % CATEGORIES)
}

/// Whether a point falls in one of the arrows that say which way the mouse goes.
fn wheel_arrow(dx: f64, dy: f64) -> bool {
    let chevron = |along: f64, across: f64, at: f64| {
        let (tip, width) = (along - at, across.abs());
        (0.0..WHEEL_ARROW_SIZE).contains(&(tip + width)) && width < WHEEL_ARROW_SIZE
    };
    chevron(-dy, dx, WHEEL_ARROW_UP)
        || chevron(dy, dx, WHEEL_ARROW_DOWN)
        || chevron(dx.abs(), dy, WHEEL_ARROW_SIDE)
}

/// The dial, with one of its sectors on, clockwise from the top.
fn wheel_dial(on: usize) -> Image {
    let mut image = Image::new(WHEEL_SIZE, WHEEL_SIZE);
    let centre = f64::from(WHEEL_SIZE - 1) / 2.0;
    for y in 0..WHEEL_SIZE {
        for x in 0..WHEEL_SIZE {
            let (dx, dy) = (f64::from(x) - centre, f64::from(y) - centre);
            let from = dx.hypot(dy);
            if from < WHEEL_INNER - 2.0 {
                let arrow = wheel_arrow(dx, dy);
                image.set(x, y, if arrow { WHEEL_ON } else { WHEEL_MIDDLE });
            } else if (WHEEL_INNER..=WHEEL_OUTER).contains(&from)
                && let Some(sector) = wheel_sector(dx, dy)
            {
                image.set(x, y, if sector == on { WHEEL_ON } else { WHEEL_OFF });
            }
        }
    }
    image
}

/// How far under the top of its line the middle of the wheel is.
pub const WHEEL_MIDDLE_DROP: f32 = WHEEL_SIZE as f32 / 2.0 + WHEEL_TOP as f32;

/// The line that draws the wheel, with `on` the sector that is on, and in its middle the
/// `choices` before, on and after, which the mouse goes through up and down.
pub fn wheel_face(
    on: usize,
    labels: [&str; CATEGORIES],
    count: &str,
    [before, chosen, after]: [&str; 3],
) -> String {
    let size = WHEEL_SIZE as i32;
    let mut pen = Pen::default();
    pen.draw(wheel_glyph(on), size + 1);
    for (sector, (middle, row)) in WHEEL_LABELS_AT.into_iter().enumerate() {
        let colour = if sector == on { "§0" } else { "§7" };
        pen.go_to(size / 2 + middle - text_width(labels[sector]) / 2);
        pen.write(row, &format!("{colour}{}", labels[sector]));
    }
    let lines = [
        (WHEEL_COUNT_ROW, format!("§7{count}")),
        (WHEEL_BEFORE_ROW, format!("§7{before}")),
        (WHEEL_CHOICE_ROW, format!("§e{chosen}")),
        (WHEEL_AFTER_ROW, format!("§7{after}")),
        (WHEEL_DONE_ROW, format!("§f{WHEEL_DONE}")),
    ];
    for (row, line) in lines {
        pen.go_to(0);
        pen.centred(size, row, &line);
    }
    pen.go_to(size);
    pen.title
}

/// A part of a soft bank of cloud, with no edge to its tiles.
fn veil_tile(column: u32, row: u32) -> Image {
    let (width, height) = VEIL_TILE;
    let mut image = Image::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let (at_x, at_y) = (f64::from(column * width + x), f64::from(row * height + y));
            let broad = fractal(VEIL_SEED, at_x, at_y * 1.6, 150.0);
            let fine = fractal(VEIL_SEED + 9, at_x, at_y * 1.6, 34.0);
            let light = (0.5 + (broad * 1.5 + fine * 0.5)).clamp(0.0, 1.0);
            let [r, g, b] = std::array::from_fn(|channel| {
                (VEIL_SHADE[channel] + (VEIL_LIGHT[channel] - VEIL_SHADE[channel]) * light) as u8
            });
            image.set(x, y, [r, g, b, 255]);
        }
    }
    image
}

/// Writes a title, keeping track of where the pen is from where it started.
#[derive(Default)]
struct Pen {
    title: String,
    at: i32,
}

impl Pen {
    /// Moves by the powers of two the font has shifts for.
    fn go_to(&mut self, x: i32) {
        let (distance, first) = match x - self.at {
            shift if shift < 0 => (-shift, FIRST_BACK),
            shift => (shift, FIRST_FORWARD),
        };
        for bit in (0..SHIFT_BITS).filter(|bit| distance & (1 << bit) != 0) {
            self.title.extend(char::from_u32(first + bit));
        }
        self.at = x;
    }

    fn draw(&mut self, glyph: char, advance: i32) {
        self.title.push_str("§f");
        self.title.push(glyph);
        self.at += advance;
    }

    /// Writes on a row of text, with `§` codes for colour.
    fn write(&mut self, row: i32, text: &str) {
        let level = text_rows()
            .iter()
            .position(|known| *known == row)
            .unwrap_or(0) as u32;
        let base = FIRST_LETTERS + level * LETTERS_PER_ROW;
        let mut code = false;
        for glyph in text.chars() {
            if code || glyph == '§' {
                self.title.push(glyph);
                code = !code;
            } else if let Some(index) = letter(glyph) {
                self.title.extend(char::from_u32(base + index as u32));
            } else {
                self.title.push(' ');
            }
        }
        self.at += text_width(text);
    }

    /// Writes centred in a span that starts at the pen's origin.
    fn centred(&mut self, span: i32, row: i32, text: &str) {
        self.go_to((span - text_width(text)) / 2);
        self.write(row, text);
    }

    /// Writes in the large letters, whose row is always the first.
    fn write_big(&mut self, text: &str) {
        let base = FIRST_LETTERS + text_rows().len() as u32 * LETTERS_PER_ROW;
        for glyph in text.chars() {
            if let Some(index) = letter(glyph) {
                self.title.extend(char::from_u32(base + index as u32));
                self.at += 2 * i32::from(LETTERS[index].advance) - 1;
            } else {
                let at = self.at + 2 * SPACE_ADVANCE;
                self.go_to(at);
            }
        }
    }
}

impl Card {
    /// The line that draws the card, with a client that has the pack.
    pub fn drawn(&self, style: Style) -> String {
        let mut pen = Pen::default();
        let name = format!("§{}{}", self.subject.title_code(), self.title);
        let lines = self.details(WIDTH as i32 - 2 * DETAIL_INDENT);
        let width = match style {
            Style::Sheet => {
                let width = SHEET_WIDTH as i32;
                pen.draw(sheet_glyph(self.subject), width + 1);
                let rows = [
                    (SHEET_HEADER_TEXT, name.clone()),
                    (SHEET_HEADER_TEXT, format!("§7{}", self.kicker)),
                    (SHEET_BODY_TEXT, self.status.clone()),
                    (SHEET_BODY_TEXT, format!("§f{}", self.description)),
                    (SHEET_BODY_TEXT, format!("§e{}", self.action)),
                ];
                for ((left, text), row) in rows.iter().zip(SHEET_ROWS) {
                    pen.go_to(*left);
                    pen.write(row, text);
                }
                width
            }
            Style::Bare => {
                let kind = format!("§7{}", self.kind);
                let width = text_width(&name).max(text_width(&kind));
                let width = lines.iter().map(|l| text_width(l)).fold(width, i32::max);
                pen.centred(width, BARE_ROWS[0], &name);
                pen.centred(width, BARE_ROWS[1], &kind);
                if self.open {
                    for (line, row) in lines.iter().zip(&BARE_ROWS[2..]) {
                        pen.centred(width, *row, line);
                    }
                }
                width
            }
            Style::Bubble => {
                let width = WIDTH as i32;
                pen.go_to((width - BUBBLE_SIZE as i32) / 2);
                pen.draw(bubble_glyph(self.subject), BUBBLE_SIZE as i32 + 1);
                pen.go_to(0);
                pen.draw(NAME_TAG, width + 1);
                pen.centred(width, NAME_TAG_TEXT, &name);
                if self.open {
                    pen.go_to(0);
                    pen.draw(DETAIL, width + 1);
                    for (line, row) in lines.iter().zip(DETAIL_LINES) {
                        pen.go_to(DETAIL_INDENT);
                        pen.write(BUBBLE_DETAIL_TOP + row, line);
                    }
                }
                width
            }
            Style::Title => {
                let kind = format!("§7{}", self.kind);
                let width = big_width(&self.title).max(FLOURISH_WIDTH as i32);
                let width = lines.iter().map(|l| text_width(l)).fold(width, i32::max);
                pen.go_to((width - big_width(&self.title)) / 2);
                pen.write_big(&self.title);
                pen.go_to((width - FLOURISH_WIDTH as i32) / 2);
                pen.draw(FLOURISH, FLOURISH_WIDTH as i32 + 1);
                pen.centred(width, TITLE_ROWS[0], &kind);
                if self.open {
                    for (line, row) in lines.iter().zip(&TITLE_ROWS[1..]) {
                        pen.centred(width, *row, line);
                    }
                }
                width
            }
        };
        // The display centres the line by where the pen ends.
        pen.go_to(width + 1);
        pen.title
    }

    /// The status, then the description cut to fit.
    fn details(&self, room: i32) -> Vec<String> {
        std::iter::once(self.status.clone())
            .chain(
                wrapped(&self.description, room)
                    .into_iter()
                    .map(|line| format!("§f{line}")),
            )
            .collect()
    }

    /// Without the pack, the glyphs would show as boxes.
    pub fn plain(&self) -> String {
        let name = format!("§{}{}", self.subject.title_code(), self.title);
        if self.open {
            format!(
                "{name}\n§7{}\n{}\n§f{}",
                self.info, self.status, self.description
            )
        } else {
            format!("{name}\n§7{}", self.info)
        }
    }

    /// How far down the drawing goes, from the top of its line.
    pub fn depth(&self, style: Style) -> i32 {
        let rows = |rows: &[i32], shown: usize| rows[shown.min(rows.len()) - 1] + LETTER_HEIGHT;
        let lines = self.details(WIDTH as i32 - 2 * DETAIL_INDENT).len();
        match style {
            Style::Sheet => SHEET_HEIGHT as i32,
            Style::Bare if self.open => rows(&BARE_ROWS, 2 + lines),
            Style::Bare => rows(&BARE_ROWS, 2),
            Style::Bubble if self.open => BUBBLE_DETAIL_TOP + DETAIL_HEIGHT as i32,
            Style::Bubble => NAME_TAG_TOP + NAME_TAG_HEIGHT as i32,
            Style::Title if self.open => rows(&TITLE_ROWS, 1 + lines),
            Style::Title => rows(&TITLE_ROWS, 1),
        }
    }
}

/// The line that shows what a region is, with its sign over its state.
pub fn sign(mark: Mark, label: &str, drawn: bool) -> String {
    if !drawn {
        return format!("{}\n§7{label}", mark.text(false));
    }
    let width = mark.image().width() as i32;
    let text = format!("§7{label}");
    let span = width.max(text_width(&text));
    let mut pen = Pen::default();
    pen.go_to((span - width) / 2);
    pen.draw(mark.glyph(), width + 1);
    pen.centred(span, SIGN_ROW, &text);
    pen.go_to(span + 1);
    pen.title
}

fn sheet_glyph(subject: Subject) -> char {
    char::from_u32(FIRST_SHEET + subject.index() as u32).unwrap_or(' ')
}

fn bubble_glyph(subject: Subject) -> char {
    char::from_u32(FIRST_BUBBLE + subject.index() as u32).unwrap_or(' ')
}

/// How far a text moves the pen in the large letters.
fn big_width(text: &str) -> i32 {
    visible(text)
        .map(|glyph| match letter(glyph) {
            Some(index) => 2 * i32::from(LETTERS[index].advance) - 1,
            None => 2 * SPACE_ADVANCE,
        })
        .sum()
}

fn bitmap(file: &str, height: i32, ascent: i32, rows: &[String]) -> String {
    format!(
        r#"{{"type":"bitmap","file":"{file}","height":{height},"ascent":{ascent},"chars":[{}]}}"#,
        rows.iter()
            .map(|row| format!("\"{row}\""))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn escaped(glyph: char) -> String {
    format!("\\u{:04X}", glyph as u32)
}

/// One drawn glyph, from a texture of its own.
fn add_glyph(
    pack: &mut ResourcePack,
    providers: &mut Vec<String>,
    name: String,
    image: &Image,
    glyph: char,
    top: i32,
) {
    pack.add(
        format!("assets/voidmap/textures/font/{name}.png"),
        image.to_png(),
    );
    providers.push(bitmap(
        &format!("voidmap:font/{name}.png"),
        image.height() as i32,
        LINE_ASCENT - top,
        &[escaped(glyph)],
    ));
}

/// A sheet's letters, renamed to private glyphs that sit on a row of text.
fn shifted_sheet(sheet: Sheet, level: usize, row: i32, zoom: i32) -> String {
    let base = FIRST_LETTERS + level as u32 * LETTERS_PER_ROW;
    let mut cells = vec![["\\u0000".to_string(), String::new()].concat(); sheet.rows() * 16];
    for (index, letter) in LETTERS.iter().enumerate() {
        if letter.sheet == sheet {
            let code = char::from_u32(base + index as u32).unwrap_or(' ');
            cells[usize::from(letter.row) * 16 + usize::from(letter.column)] = escaped(code);
        }
    }
    let rows: Vec<String> = cells.chunks(16).map(|row| row.concat()).collect();
    let (height, ascent) = sheet.metrics();
    bitmap(sheet.file(), height * zoom, ascent * zoom - row, &rows)
}

fn shifts() -> String {
    let advances: Vec<String> = (0..SHIFT_BITS)
        .flat_map(|bit| {
            let step = 1 << bit;
            [(FIRST_FORWARD + bit, step), (FIRST_BACK + bit, -step)]
        })
        .map(|(code, step)| format!(r#""\u{code:04X}":{step}"#))
        .collect();
    format!(
        r#"{{"type":"space","advances":{{{}}}}}"#,
        advances.join(",")
    )
}

/// Adds the panels and the font that draws them.
pub fn install(pack: &mut ResourcePack) {
    cursor::install(pack);
    settings::install(pack);
    transition::install(pack, crate::plugin::FIRST_MODEL_BLOCK);
    pack.add(EYE_SKIN_FILE, Image::new(SKIN_SIZE, SKIN_SIZE).to_png());
    let mut providers = vec![shifts()];
    for subject in SUBJECTS {
        let index = subject.index();
        add_glyph(
            pack,
            &mut providers,
            format!("sheet_{index}"),
            &sheet(subject),
            sheet_glyph(subject),
            0,
        );
        add_glyph(
            pack,
            &mut providers,
            format!("bubble_{index}"),
            &bubble(subject),
            bubble_glyph(subject),
            0,
        );
    }
    let shapes = [
        (
            "name_tag",
            rounded(WIDTH, NAME_TAG_HEIGHT),
            NAME_TAG,
            NAME_TAG_TOP,
        ),
        (
            "detail",
            rounded(WIDTH, DETAIL_HEIGHT),
            DETAIL,
            BUBBLE_DETAIL_TOP,
        ),
        ("flourish", flourish(), FLOURISH, FLOURISH_TOP),
    ];
    for (name, image, glyph, top) in shapes {
        add_glyph(pack, &mut providers, name.to_string(), &image, glyph, top);
    }
    for mark in MARKS {
        let name = format!("mark_{}", mark.index());
        add_glyph(pack, &mut providers, name, &mark.image(), mark.glyph(), 0);
    }
    for sector in 0..CATEGORIES {
        add_glyph(
            pack,
            &mut providers,
            format!("wheel_{sector}"),
            &wheel_dial(sector),
            wheel_glyph(sector),
            WHEEL_TOP,
        );
    }
    for row in 0..VEIL_ROWS {
        // One row over the middle of the screen, one under it.
        let top = LINE_ASCENT - (VEIL_TILE.1 as i32 - 3) + row as i32 * VEIL_TILE.1 as i32;
        for column in 0..VEIL_COLUMNS {
            add_glyph(
                pack,
                &mut providers,
                format!("veil_{row}_{column}"),
                &veil_tile(column, row),
                veil_glyph(column, row),
                top,
            );
        }
    }
    let rows = text_rows();
    for (level, row) in rows.iter().enumerate() {
        for sheet in SHEETS {
            providers.push(shifted_sheet(sheet, level, *row, 1));
        }
    }
    for sheet in SHEETS {
        providers.push(shifted_sheet(sheet, rows.len(), 0, 2));
    }
    let font = format!(r#"{{"providers":[{}]}}"#, providers.join(","));
    pack.add(FONT, font.into_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::regions::Regions;

    fn card(subject: Subject, open: bool) -> Card {
        Card {
            subject,
            title: "Port-Noir".into(),
            kind: "Phare".into(),
            kicker: "Phare - Côte Ouest".into(),
            info: "Phare · 100 blocs".into(),
            status: "§aCôte Ouest".into(),
            description: "Son feu se voit de loin.".into(),
            action: "Clic gauche : sélectionner".into(),
            open,
        }
    }

    /// Adds up what the line's private glyphs and letters advance the pen by.
    fn advance(title: &str) -> i32 {
        let rows = text_rows().len() as u32;
        let mut total = 0;
        let mut code = false;
        for glyph in title.chars() {
            let value = glyph as u32;
            if code || glyph == '§' {
                code = !code;
            } else if (FIRST_FORWARD..FIRST_FORWARD + SHIFT_BITS).contains(&value) {
                total += 1 << (value - FIRST_FORWARD);
            } else if (FIRST_BACK..FIRST_BACK + SHIFT_BITS).contains(&value) {
                total -= 1 << (value - FIRST_BACK);
            } else if value >= FIRST_LETTERS {
                let index = ((value - FIRST_LETTERS) % LETTERS_PER_ROW) as usize;
                let letter = i32::from(LETTERS[index].advance);
                let big = (value - FIRST_LETTERS) / LETTERS_PER_ROW == rows;
                total += if big { 2 * letter - 1 } else { letter };
            } else if value < FIRST_SHEET {
                total += text_width(&glyph.to_string());
            } else {
                total += glyph_advance(glyph);
            }
        }
        total
    }

    fn glyph_advance(glyph: char) -> i32 {
        let value = glyph as u32;
        1 + if (FIRST_SHEET..FIRST_BUBBLE).contains(&value) {
            SHEET_WIDTH as i32
        } else if (FIRST_BUBBLE..NAME_TAG as u32).contains(&value) {
            BUBBLE_SIZE as i32
        } else if glyph == FLOURISH {
            FLOURISH_WIDTH as i32
        } else if value >= FIRST_MARK {
            MARKS[(value - FIRST_MARK) as usize].image().width() as i32
        } else {
            WIDTH as i32
        }
    }

    /// The display centres a line by where the pen ends, so it has to be the widest
    /// thing drawn: no style may end short of what it put on the line.
    #[test]
    fn a_drawn_card_ends_past_all_it_drew() {
        for style in STYLES {
            for open in [false, true] {
                let drawn = card(Subject::You, open).drawn(style);
                assert!(advance(&drawn) >= 40, "{style:?} {open}");
            }
        }
    }

    #[test]
    fn a_card_without_the_pack_has_no_private_glyphs() {
        let plain = card(Subject::You, true).plain();
        assert!(
            plain
                .chars()
                .all(|c| !('\u{E000}'..='\u{F8FF}').contains(&c))
        );
        assert!(plain.contains("Port-Noir"));
    }

    #[test]
    fn text_width_counts_what_the_font_draws() {
        assert_eq!(text_width("il"), 2 + 3);
        assert_eq!(text_width("a b"), 6 + SPACE_ADVANCE + 6);
        assert_eq!(text_width("é"), 6, "accents are in another sheet");
        assert_eq!(text_width("§aa§r"), 6, "codes take no room");
    }

    #[test]
    fn a_long_description_wraps_within_the_detail_panel() {
        let room = WIDTH as i32 - 2 * DETAIL_INDENT;
        let lines = wrapped("Des maisons, un marché, du monde.", room);
        assert!(lines.len() < DETAIL_LINES.len(), "{lines:?}");
        assert!(lines.iter().all(|line| text_width(line) <= room));
    }

    #[test]
    fn every_region_name_fits_on_its_line() {
        let room = WIDTH as i32 - 2 * DETAIL_INDENT;
        let regions = Regions::demo();
        for x in (-400..400).step_by(20) {
            for z in (-400..400).step_by(20) {
                let name = regions.at(f64::from(x), f64::from(z)).name;
                assert!(text_width(name) <= room, "{name}");
            }
        }
    }

    /// The sheet says it all, as it did. The others keep it for a selected place.
    #[test]
    fn the_details_only_show_once_selected_except_on_a_sheet() {
        let closed = card(Subject::You, false);
        let open = card(Subject::You, true);
        for style in [Style::Bare, Style::Bubble, Style::Title] {
            assert_ne!(closed.drawn(style), open.drawn(style), "{style:?}");
            assert!(closed.depth(style) < open.depth(style), "{style:?}");
        }
        assert_eq!(closed.drawn(Style::Sheet), open.drawn(Style::Sheet));
    }

    #[test]
    fn styles_cycle_through_each_other() {
        let mut style = Style::Sheet;
        for _ in 0..STYLES.len() {
            style = style.next();
        }
        assert_eq!(style, Style::Sheet);
        assert_ne!(Style::Sheet.next(), Style::Sheet);
    }

    #[test]
    fn panels_take_the_colour_of_their_subject() {
        let mine = sheet(Subject::Place(MarkerKind::Mine));
        let you = sheet(Subject::You);
        assert_ne!(
            mine.pixel(SHEET_WIDTH / 2, 8),
            you.pixel(SHEET_WIDTH / 2, 8)
        );
    }

    #[test]
    fn a_drawn_glyph_keeps_its_full_width_for_the_font() {
        let image = sheet(Subject::You);
        assert_ne!(image.pixel(SHEET_WIDTH - 1, SHEET_HEIGHT / 2)[3], 0);
        let image = bubble(Subject::You);
        assert_ne!(image.pixel(BUBBLE_SIZE - 1, BUBBLE_SIZE / 2)[3], 0);
    }

    #[test]
    fn the_letters_of_every_row_stay_within_the_private_area() {
        let last = FIRST_LETTERS + (text_rows().len() as u32 + 1) * LETTERS_PER_ROW;
        assert!(last <= 0xF8FF);
        assert!(LETTERS.len() <= LETTERS_PER_ROW as usize);
    }

    #[test]
    fn a_sign_says_its_state_with_or_without_the_pack() {
        assert!(sign(Mark::Lock, "Verrouillée", false).contains("Verrouillée"));
        assert!(sign(Mark::Lock, "Verrouillée", true).contains(Mark::Lock.glyph()));
    }

    #[test]
    fn the_font_lists_every_panel() {
        let mut pack = ResourcePack::new("test");
        install(&mut pack);
        assert!(pack.contains(FONT));
        for subject in SUBJECTS {
            let index = subject.index();
            assert!(pack.contains(&format!("assets/voidmap/textures/font/sheet_{index}.png")));
            assert!(pack.contains(&format!("assets/voidmap/textures/font/bubble_{index}.png")));
        }
    }
}
