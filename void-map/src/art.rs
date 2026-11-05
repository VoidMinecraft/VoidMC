//! The info card: a drawn panel behind text, from the resource pack.
//!
//! A text display takes a plain string, so the panel's glyphs join the default font.
//! Fonts of the same name add up across packs, and the client's letters stay.

use voidmc_ui::{Image, ResourcePack};

use crate::markers::MarkerKind;

const WIDTH: u32 = 176;
const HEIGHT: u32 = 60;
const HEADER: u32 = 25;
/// Text starts this far in, past the icon in the header.
const HEADER_TEXT: i32 = 27;
const BODY_TEXT: i32 = 8;
/// From the baseline of the first line, which stays empty and sets the card's width.
const ASCENT: u32 = 3;
const ICON_AT: (u32, u32) = (6, 6);

const FIRST_GLYPH: u32 = 0xE100;
const HEADER_PAD: char = '\u{E110}';
const BODY_PAD: char = '\u{E111}';
const FONT: &str = "assets/minecraft/font/default.json";
const MINECART: &str = "assets/minecraft/textures/entity/minecart/minecart.png";
const MINECART_SIZE: (u32, u32) = (64, 32);

const FILL: [u8; 4] = [17, 21, 30, 238];
const EDGE: [u8; 4] = [6, 8, 12, 255];

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

    fn glyph(self) -> char {
        char::from_u32(FIRST_GLYPH + self.index() as u32).unwrap_or(' ')
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

fn panel(subject: Subject) -> Image {
    let accent = subject.accent();
    let mut image = Image::new(WIDTH, HEIGHT);
    let (right, bottom) = (WIDTH - 1, HEIGHT - 1);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let edge = x.min(right - x).min(y.min(bottom - y));
            let corner = x.min(right - x) + y.min(bottom - y);
            let color = match (corner, edge) {
                (0 | 1, _) => continue,
                (2, _) | (_, 0) => EDGE,
                (3, _) | (_, 1) => blend(accent, [0, 0, 0, 255], 0.9),
                _ if y == HEADER => blend(accent, FILL, 0.55),
                _ if y < HEADER => {
                    let fade = 0.34 - 0.2 * f64::from(y) / f64::from(HEADER);
                    blend(accent, FILL, fade)
                }
                _ => FILL,
            };
            image.set(x, y, color);
        }
    }
    let lit = |rgb: Rgb| [rgb[0], rgb[1], rgb[2], 255];
    for (row, line) in subject.icon().iter().enumerate() {
        for (column, pixel) in line.bytes().enumerate() {
            let color = match pixel {
                b'#' => lit(accent),
                b'o' => [236, 238, 242, 255],
                _ => continue,
            };
            image.set(ICON_AT.0 + column as u32, ICON_AT.1 + row as u32 - 1, color);
        }
    }
    image
}

/// The lines of a card, most important first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub subject: Subject,
    pub title: String,
    pub kicker: String,
    pub status: String,
    pub description: String,
    pub action: String,
}

impl Card {
    /// With a client that has the pack.
    pub fn drawn(&self) -> String {
        format!(
            "{panel}\n{HEADER_PAD}§{code}§l{title}§r\n{HEADER_PAD}§7{kicker}\n\
             {BODY_PAD}{status}\n{BODY_PAD}§f{description}\n{BODY_PAD}§e{action}",
            panel = self.subject.glyph(),
            code = self.subject.title_code(),
            title = self.title,
            kicker = self.kicker,
            status = self.status,
            description = self.description,
            action = self.action,
        )
    }

    /// Without the pack, the glyphs would show as boxes.
    pub fn plain(&self) -> String {
        format!(
            "§{code}§l{title}§r\n§7{kicker}\n{status}\n§f{description}\n§e{action}",
            code = self.subject.title_code(),
            title = self.title,
            kicker = self.kicker,
            status = self.status,
            description = self.description,
            action = self.action,
        )
    }
}

/// Width of the drawn card, in pixels of text.
pub const CARD_WIDTH: i32 = WIDTH as i32 + 1;

/// Adds the panels and the font that draws them. Also turns minecarts black, for the
/// one around the eye: looking up at it would otherwise show its floor on the black sky.
pub fn install(pack: &mut ResourcePack) {
    let black = Image::filled(MINECART_SIZE.0, MINECART_SIZE.1, [0, 0, 0, 255]);
    pack.add(MINECART, black.to_png());
    let mut providers = Vec::new();
    for (index, subject) in SUBJECTS.iter().enumerate() {
        let name = format!("card_{index}");
        pack.add(
            format!("assets/voidmap/textures/font/{name}.png"),
            panel(*subject).to_png(),
        );
        providers.push(format!(
            r#"{{"type":"bitmap","file":"voidmap:font/{name}.png","height":{HEIGHT},"ascent":{ASCENT},"chars":["\u{:04X}"]}}"#,
            FIRST_GLYPH + index as u32
        ));
    }
    providers.push(format!(
        r#"{{"type":"space","advances":{{"\u{:04X}":{HEADER_TEXT},"\u{:04X}":{BODY_TEXT}}}}}"#,
        HEADER_PAD as u32, BODY_PAD as u32
    ));
    let font = format!(r#"{{"providers":[{}]}}"#, providers.join(","));
    pack.add(FONT, font.into_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(subject: Subject) -> Card {
        Card {
            subject,
            title: "Port-Noir".into(),
            kicker: "Phare - Cote Ouest".into(),
            status: "§aDecouverte§7 - 100 blocs".into(),
            description: "Son feu se voit de loin.".into(),
            action: "Clic : selectionner".into(),
        }
    }

    #[test]
    fn every_subject_has_its_own_panel_glyph() {
        let mut glyphs: Vec<_> = SUBJECTS.iter().map(|subject| subject.glyph()).collect();
        glyphs.dedup();
        assert_eq!(glyphs.len(), SUBJECTS.len());
        assert!(glyphs.iter().all(|glyph| *glyph < HEADER_PAD));
    }

    #[test]
    fn panels_take_the_colour_of_their_subject() {
        let header = |subject| panel(subject).pixel(WIDTH / 2, 8);
        assert_ne!(
            header(Subject::Place(MarkerKind::Mine)),
            header(Subject::You)
        );
    }

    #[test]
    fn a_panel_keeps_its_full_width_for_the_font() {
        let image = panel(Subject::You);
        assert_ne!(image.pixel(0, HEIGHT / 2)[3], 0);
        assert_ne!(image.pixel(WIDTH - 1, HEIGHT / 2)[3], 0);
    }

    #[test]
    fn a_card_without_the_pack_has_no_private_glyphs() {
        let plain = card(Subject::You).plain();
        assert!(
            plain
                .chars()
                .all(|c| !('\u{E000}'..='\u{F8FF}').contains(&c))
        );
        assert!(plain.contains("Port-Noir"));
    }

    #[test]
    fn a_drawn_card_starts_with_its_panel_alone_on_a_line() {
        let subject = Subject::Place(MarkerKind::Ruins);
        let drawn = card(subject).drawn();
        assert_eq!(
            drawn.lines().next(),
            Some(subject.glyph().to_string().as_str())
        );
    }

    #[test]
    fn the_font_lists_every_panel() {
        let mut pack = ResourcePack::new("test");
        install(&mut pack);
        assert!(pack.contains(FONT));
        for index in 0..SUBJECTS.len() {
            assert!(pack.contains(&format!("assets/voidmap/textures/font/card_{index}.png")));
        }
    }
}
