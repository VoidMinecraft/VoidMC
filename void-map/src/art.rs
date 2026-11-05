//! The info panel: drawn glyphs in one line of a text display, stuck to its marker.
//!
//! Glyphs can draw anything and move the pen, so one line of text places the panel and
//! its text to the pixel. The panels and shifted copies of the client's letters join
//! the default font: fonts of the same name add up across packs, and the client's
//! letters stay. A glyph cannot rise above its line, so everything hangs below it.

use voidmc_ui::{Image, ResourcePack};

use crate::alphabet::LETTERS;
use crate::markers::MarkerKind;

const WIDTH: u32 = 150;
const DETAIL_GAP: i32 = 2;
const DETAIL_HEIGHT: u32 = 38;
const DETAIL_INDENT: i32 = 8;
const DETAIL_LINES: [i32; 3] = [6, 17, 28];
/// How far the tag's point reaches down from its body, towards the marker.
const TAG_POINT: u32 = 6;

/// A line of text has its top this far above where the bitmap of a letter sits.
const LINE_ASCENT: i32 = 7;
/// Accented letters come in taller cells, whose top stands higher.
const ACCENT_ASCENT: i32 = 10;
const ACCENT_HEIGHT: i32 = 12;
const LETTER_HEIGHT: i32 = 8;
/// What a space takes in the default font.
const SPACE_ADVANCE: i32 = 4;

const FIRST_PANEL: u32 = 0xE100;
const PANELS_PER_STYLE: u32 = 8;
const FIRST_DETAIL: u32 = 0xE120;
const FIRST_FORWARD: u32 = 0xE200;
const FIRST_BACK: u32 = 0xE220;
const SHIFT_BITS: u32 = 10;
const FIRST_LETTERS: u32 = 0xE400;
const LETTERS_PER_ROW: u32 = 0x100;
const FONT: &str = "assets/minecraft/font/default.json";
const MINECART: &str = "assets/minecraft/textures/entity/minecart/minecart.png";
const MINECART_SIZE: (u32, u32) = (64, 32);

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
    /// Icon and name.
    #[default]
    Card,
    /// A thin strip: name, and the kind of place at its end.
    Band,
    /// A tag pointing at its marker: icon, name and one line of information.
    Tag,
}

const STYLES: [Style; 3] = [Style::Card, Style::Band, Style::Tag];

impl Style {
    pub fn label(self) -> &'static str {
        match self {
            Self::Card => "Carte",
            Self::Band => "Bandeau",
            Self::Tag => "Étiquette",
        }
    }

    pub fn next(self) -> Self {
        STYLES[(self.index() + 1) % STYLES.len()]
    }

    fn index(self) -> usize {
        STYLES.iter().position(|style| *style == self).unwrap_or(0)
    }

    fn height(self) -> u32 {
        match self {
            Self::Card => 24,
            Self::Band => 13,
            Self::Tag => 28 + TAG_POINT,
        }
    }

    /// Rows of its text, from the top of the panel.
    fn lines(self) -> &'static [i32] {
        match self {
            Self::Card => &[8],
            Self::Band => &[3],
            Self::Tag => &[6, 17],
        }
    }

    fn icon_at(self) -> Option<(u32, u32)> {
        match self {
            Self::Card => Some((5, 4)),
            Self::Band => None,
            Self::Tag => Some((5, 6)),
        }
    }

    /// Where its text starts in the panel.
    fn indent(self) -> i32 {
        match self {
            Self::Card => 26,
            Self::Band => 9,
            Self::Tag => 26,
        }
    }

    fn panel_glyph(self, subject: Subject) -> char {
        let code = FIRST_PANEL + self.index() as u32 * PANELS_PER_STYLE + subject.index() as u32;
        char::from_u32(code).unwrap_or(' ')
    }

    fn detail_glyph(self) -> char {
        char::from_u32(FIRST_DETAIL + self.index() as u32).unwrap_or(' ')
    }

    fn detail_top(self) -> i32 {
        self.height() as i32 + DETAIL_GAP
    }

    /// Whether a pixel of the panel belongs to its shape.
    fn covers(self, x: i32, y: i32) -> bool {
        let (width, height) = (WIDTH as i32, self.height() as i32);
        if x < 0 || y < 0 || x >= width || y >= height {
            return false;
        }
        let from_end = (width - 1 - x).min(x);
        let from_side = (height - 1 - y).min(y);
        match self {
            Self::Card | Self::Band => from_end + from_side >= 2,
            Self::Tag => {
                let body = height - TAG_POINT as i32;
                if y < body {
                    from_end + y.min(body - 1 - y) >= 2
                } else {
                    (2 * x - (width - 1)).abs() <= 2 * (height - 1 - y) + 1
                }
            }
        }
    }
}

/// Every row of text any panel writes on, from the top of the line.
fn text_rows() -> Vec<i32> {
    let mut rows: Vec<i32> = STYLES
        .iter()
        .flat_map(|style| {
            let own = style.lines().iter().copied();
            let detail = DETAIL_LINES.iter().map(|line| style.detail_top() + line);
            own.chain(detail)
        })
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

fn panel(style: Style, subject: Subject) -> Image {
    let accent = subject.accent();
    let mut image = Image::new(WIDTH, style.height());
    for y in 0..style.height() as i32 {
        for x in 0..WIDTH as i32 {
            if !style.covers(x, y) {
                continue;
            }
            let rim = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .any(|(dx, dy)| !style.covers(x + dx, y + dy));
            let color = match style {
                _ if rim => EDGE,
                Style::Band if x <= 4 => opaque(accent),
                Style::Tag if y == 1 => opaque(accent),
                Style::Card if y == style.height() as i32 - 2 => blend(accent, FILL, 0.7),
                _ => FILL,
            };
            image.set(x as u32, y as u32, color);
        }
    }
    if let Some((left, top)) = style.icon_at() {
        for (row, line) in subject.icon().iter().enumerate() {
            for (column, pixel) in line.bytes().enumerate() {
                let color = match pixel {
                    b'#' => opaque(accent),
                    b'o' => [236, 238, 242, 255],
                    _ => continue,
                };
                image.set(left + column as u32, top + row as u32 - 1, color);
            }
        }
    }
    image
}

/// What only a selected place shows, under its panel.
fn detail_panel() -> Image {
    let mut image = Image::new(WIDTH, DETAIL_HEIGHT);
    let (right, bottom) = (WIDTH - 1, DETAIL_HEIGHT - 1);
    for y in 0..DETAIL_HEIGHT {
        for x in 0..WIDTH {
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

/// What the panel says about the place under the look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub subject: Subject,
    pub title: String,
    pub kind: String,
    /// One line, for the styles that have room for it.
    pub info: String,
    pub status: String,
    pub description: String,
    /// The place is selected, so the panel shows all of it.
    pub open: bool,
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
}

impl Card {
    /// The line that draws the card, with a client that has the pack.
    pub fn drawn(&self, style: Style) -> String {
        let mut pen = Pen::default();
        let title = format!("§{}{}", self.subject.title_code(), self.title);
        let width = WIDTH as i32;
        pen.draw(style.panel_glyph(self.subject), width + 1);
        pen.go_to(style.indent());
        pen.write(style.lines()[0], &title);
        match style {
            Style::Card => {}
            Style::Band => {
                let kind = format!("§7{}", self.kind);
                pen.go_to(width - 6 - text_width(&kind));
                pen.write(style.lines()[0], &kind);
            }
            Style::Tag => {
                pen.go_to(style.indent());
                pen.write(style.lines()[1], &format!("§7{}", self.info));
            }
        }
        if self.open {
            let top = style.detail_top();
            pen.go_to(0);
            pen.draw(style.detail_glyph(), width + 1);
            let room = width - 2 * DETAIL_INDENT;
            let lines = std::iter::once(self.status.clone()).chain(
                wrapped(&self.description, room)
                    .into_iter()
                    .map(|line| format!("§f{line}")),
            );
            for (line, row) in lines.zip(DETAIL_LINES) {
                pen.go_to(DETAIL_INDENT);
                pen.write(top + row, &line);
            }
        }
        // The display centres the line by where the pen ends.
        pen.go_to(width + 1);
        pen.title
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
        if self.open {
            style.detail_top() + DETAIL_HEIGHT as i32
        } else {
            style.height() as i32
        }
    }
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
fn shifted_sheet(sheet: Sheet, level: usize, row: i32) -> String {
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
    bitmap(sheet.file(), height, ascent - row, &rows)
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

/// Adds the panels and the font that draws them. Also turns minecarts black, for the
/// one around the eye: looking up at it would otherwise show its floor on the black sky.
pub fn install(pack: &mut ResourcePack) {
    let black = Image::filled(MINECART_SIZE.0, MINECART_SIZE.1, [0, 0, 0, 255]);
    pack.add(MINECART, black.to_png());
    let mut providers = vec![shifts()];
    for style in STYLES {
        for subject in SUBJECTS {
            add_glyph(
                pack,
                &mut providers,
                format!("panel_{}_{}", style.index(), subject.index()),
                &panel(style, subject),
                style.panel_glyph(subject),
                0,
            );
        }
        add_glyph(
            pack,
            &mut providers,
            format!("detail_{}", style.index()),
            &detail_panel(),
            style.detail_glyph(),
            style.detail_top(),
        );
    }
    for (level, row) in text_rows().into_iter().enumerate() {
        for sheet in SHEETS {
            providers.push(shifted_sheet(sheet, level, row));
        }
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
            info: "Phare · 100 blocs".into(),
            status: "§aCôte Ouest".into(),
            description: "Son feu se voit de loin.".into(),
            open,
        }
    }

    /// Adds up what the line's private glyphs and letters advance the pen by.
    fn advance(title: &str) -> i32 {
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
            } else if (FIRST_PANEL..FIRST_FORWARD).contains(&value) {
                total += WIDTH as i32 + 1;
            } else if value >= FIRST_LETTERS {
                let index = ((value - FIRST_LETTERS) % LETTERS_PER_ROW) as usize;
                total += i32::from(LETTERS[index].advance);
            } else {
                total += text_width(&glyph.to_string());
            }
        }
        total
    }

    #[test]
    fn a_card_ends_where_its_panel_does_whatever_it_writes() {
        for style in STYLES {
            for open in [false, true] {
                let drawn = card(Subject::You, open).drawn(style);
                assert_eq!(advance(&drawn), WIDTH as i32 + 1, "{style:?} {open}");
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

    #[test]
    fn the_details_only_show_once_selected() {
        let glyph = Style::Card.detail_glyph();
        let closed = card(Subject::You, false).drawn(Style::Card);
        let open = card(Subject::You, true).drawn(Style::Card);
        assert!(!closed.contains(glyph));
        assert!(open.contains(glyph));
    }

    #[test]
    fn styles_cycle_through_each_other() {
        assert_eq!(Style::Card.next().next().next(), Style::Card);
        assert_ne!(Style::Card.next(), Style::Card);
    }

    #[test]
    fn every_panel_has_its_own_glyph() {
        let mut glyphs: Vec<_> = STYLES
            .iter()
            .flat_map(|style| SUBJECTS.map(|subject| style.panel_glyph(subject)))
            .chain(STYLES.iter().map(|style| style.detail_glyph()))
            .collect();
        let count = glyphs.len();
        glyphs.sort_unstable();
        glyphs.dedup();
        assert_eq!(glyphs.len(), count);
    }

    #[test]
    fn a_panel_keeps_its_full_width_for_the_font() {
        for style in STYLES {
            let image = panel(style, Subject::You);
            assert_ne!(
                image.pixel(WIDTH - 1, style.height() / 2)[3],
                0,
                "{style:?}"
            );
        }
    }

    #[test]
    fn panels_take_the_colour_of_their_subject() {
        let mine = panel(Style::Band, Subject::Place(MarkerKind::Mine));
        let you = panel(Style::Band, Subject::You);
        assert_ne!(mine.pixel(3, 6), you.pixel(3, 6));
    }

    #[test]
    fn the_letters_of_every_row_stay_within_the_private_area() {
        let last = FIRST_LETTERS + text_rows().len() as u32 * LETTERS_PER_ROW;
        assert!(last <= 0xF8FF);
        assert!(LETTERS.len() <= LETTERS_PER_ROW as usize);
    }

    #[test]
    fn the_font_lists_every_panel() {
        let mut pack = ResourcePack::new("test");
        install(&mut pack);
        assert!(pack.contains(FONT));
        for style in STYLES {
            for subject in SUBJECTS {
                let name = format!("panel_{}_{}", style.index(), subject.index());
                assert!(pack.contains(&format!("assets/voidmap/textures/font/{name}.png")));
            }
        }
    }
}
