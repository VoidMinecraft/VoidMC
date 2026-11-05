//! The board's pictures, drawn when the server starts: lit from the top left and
//! seen slightly from above, which gives flat tiles some depth.

use voidmc_ui::{Cursor, FontBuilder, Image, ResourcePack, Text, Tile};

use crate::game::{Color, Piece};

const TILE: u32 = 18;
const CENTRE: f64 = (TILE - 1) as f64 / 2.0;
/// Squashes circles vertically, as seen from an angle.
const PERSPECTIVE: f64 = 1.15;
const RADIUS: f64 = 6.8;
const GHOST_ALPHA: f64 = 0.62;

type Rgb = [u8; 3];

const LIGHT: Rgb = [226, 200, 160];
const DARK: Rgb = [112, 72, 46];
const TARGET: Rgb = [130, 214, 96];
const WOOD: Rgb = [66, 42, 28];
const TRAY: Rgb = [84, 56, 38];
/// Width of the wooden edge between the board and the trays.
const EDGE: u32 = 6;
const GOLD: Rgb = [240, 192, 56];
const WHITE: Palette = Palette {
    top: [242, 236, 224],
    side: [176, 164, 146],
};
const BLACK: Palette = Palette {
    top: [70, 56, 56],
    side: [30, 24, 24],
};

#[derive(Clone, Copy)]
struct Palette {
    top: Rgb,
    side: Rgb,
}

fn shade(color: Rgb, amount: i16) -> Rgb {
    color.map(|channel| (i16::from(channel) + amount).clamp(0, 255) as u8)
}

fn blend(under: [u8; 4], over: Rgb, alpha: f64) -> [u8; 4] {
    if under[3] == 0 {
        return [over[0], over[1], over[2], (alpha * 255.0).round() as u8];
    }
    let mix = |u: u8, o: u8| (f64::from(u) + (f64::from(o) - f64::from(u)) * alpha).round() as u8;
    [
        mix(under[0], over[0]),
        mix(under[1], over[1]),
        mix(under[2], over[2]),
        under[3],
    ]
}

fn disc(image: &mut Image, cx: f64, cy: f64, radius: f64, color: Rgb, alpha: f64) {
    for y in 0..TILE {
        for x in 0..TILE {
            let (dx, dy) = (f64::from(x) - cx, (f64::from(y) - cy) * PERSPECTIVE);
            if dx * dx + dy * dy <= radius * radius {
                image.set(x, y, blend(image.pixel(x, y), color, alpha));
            }
        }
    }
}

fn square(color: Rgb) -> Image {
    let [r, g, b] = color;
    let mut image = Image::filled(TILE, TILE, [r, g, b, 255]);
    let [lr, lg, lb] = shade(color, 22);
    let [dr, dg, db] = shade(color, -26);
    for i in 0..TILE {
        image.set(i, 0, [lr, lg, lb, 255]);
        image.set(0, i, [lr, lg, lb, 255]);
        image.set(i, TILE - 1, [dr, dg, db, 255]);
        image.set(TILE - 1, i, [dr, dg, db, 255]);
    }
    image
}

fn draw_piece(image: &mut Image, palette: Palette, king: bool, alpha: f64) {
    for offset in [2.0, 1.0] {
        disc(image, CENTRE, CENTRE + offset, RADIUS, palette.side, alpha);
    }
    disc(image, CENTRE, CENTRE, RADIUS, palette.top, alpha);
    disc(image, CENTRE, CENTRE, 4.6, shade(palette.top, -18), alpha);
    disc(image, CENTRE - 1.0, CENTRE - 1.0, 3.4, palette.top, alpha);
    if king {
        for offset in [-1.0, -2.0] {
            disc(image, CENTRE, CENTRE + offset, 5.6, palette.side, alpha);
        }
        disc(image, CENTRE, CENTRE - 3.0, 5.6, palette.top, alpha);
        disc(image, CENTRE, CENTRE - 3.0, 2.8, GOLD, alpha);
    }
}

fn piece_tile(palette: Palette, king: bool) -> Image {
    let mut image = square(DARK);
    disc(&mut image, CENTRE + 1.0, CENTRE + 3.0, 7.0, [0, 0, 0], 0.35);
    draw_piece(&mut image, palette, king, 1.0);
    image
}

fn target_tile() -> Image {
    let mut image = square(DARK);
    disc(&mut image, CENTRE, CENTRE, 4.0, shade(TARGET, -70), 1.0);
    disc(&mut image, CENTRE, CENTRE - 1.0, 3.0, TARGET, 1.0);
    image
}

/// Where the held piece was: a hollow in the board.
fn socket_tile() -> Image {
    let mut image = square(DARK);
    disc(&mut image, CENTRE, CENTRE, RADIUS, shade(DARK, -36), 1.0);
    image
}

fn ghost(palette: Palette, king: bool) -> Image {
    let mut image = Image::new(TILE, TILE);
    draw_piece(&mut image, palette, king, GHOST_ALPHA);
    image
}

fn edge() -> Image {
    let [r, g, b] = WOOD;
    let mut image = Image::filled(EDGE, TILE, [r, g, b, 255]);
    let [dr, dg, db] = shade(WOOD, -14);
    for y in (0..TILE).step_by(3) {
        image.set(y % EDGE, y, [dr, dg, db, 255]);
    }
    image
}

/// A slot of the tray that holds taken pieces, sunk into the wood.
fn tray() -> Image {
    let [r, g, b] = TRAY;
    let mut image = Image::filled(TILE, TILE, [r, g, b, 255]);
    let [dr, dg, db] = shade(TRAY, -22);
    for i in 0..TILE {
        image.set(i, 0, [dr, dg, db, 255]);
        image.set(0, i, [dr, dg, db, 255]);
    }
    image
}

fn tray_piece(color: Color) -> Image {
    let mut image = tray();
    let palette = palette(color);
    disc(&mut image, CENTRE, CENTRE + 1.0, 5.4, palette.side, 1.0);
    disc(&mut image, CENTRE, CENTRE, 5.4, palette.top, 1.0);
    image
}

fn palette(color: Color) -> Palette {
    match color {
        Color::White => WHITE,
        Color::Black => BLACK,
    }
}

const PIECES: [Piece; 4] = [
    Piece {
        color: Color::White,
        king: false,
    },
    Piece {
        color: Color::White,
        king: true,
    },
    Piece {
        color: Color::Black,
        king: false,
    },
    Piece {
        color: Color::Black,
        king: true,
    },
];

fn index(piece: Piece) -> usize {
    PIECES
        .iter()
        .position(|candidate| *candidate == piece)
        .expect("PIECES lists every piece")
}

/// Everything the board is drawn with.
#[derive(bevy_ecs::resource::Resource)]
pub struct Art {
    pub font: String,
    pub light: Tile,
    pub dark: Tile,
    pub target: Tile,
    pub socket: Tile,
    pub edge: Tile,
    pub tray: Tile,
    tray_white: Tile,
    tray_black: Tile,
    pieces: Vec<Tile>,
    ghosts: Vec<Cursor>,
}

impl Art {
    /// Draws the art and adds it to the pack.
    pub fn new(pack: &mut ResourcePack) -> Self {
        let mut font = FontBuilder::new("voidmc", "checkers");
        let art = Self {
            font: font.id(),
            light: font.tile(square(LIGHT)),
            dark: font.tile(square(DARK)),
            target: font.tile(target_tile()),
            socket: font.tile(socket_tile()),
            edge: font.tile(edge()),
            tray: font.tile(tray()),
            tray_white: font.tile(tray_piece(Color::White)),
            tray_black: font.tile(tray_piece(Color::Black)),
            pieces: PIECES
                .iter()
                .map(|p| font.tile(piece_tile(palette(p.color), p.king)))
                .collect(),
            ghosts: PIECES
                .iter()
                .map(|p| font.cursor(ghost(palette(p.color), p.king)))
                .collect(),
        };
        font.install(pack);
        pack.hide_tooltip_frames();
        art
    }

    pub fn taken(&self, color: Color) -> &Tile {
        match color {
            Color::White => &self.tray_white,
            Color::Black => &self.tray_black,
        }
    }

    pub fn piece(&self, piece: Piece) -> &Tile {
        &self.pieces[index(piece)]
    }

    pub fn ghost(&self, piece: Piece) -> Text {
        self.ghosts[index(piece)].text()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_piece_has_its_own_tile() {
        let art = Art::new(&mut ResourcePack::new("test"));
        for (i, a) in PIECES.iter().enumerate() {
            for b in &PIECES[i + 1..] {
                assert_ne!(art.piece(*a), art.piece(*b));
            }
        }
    }

    #[test]
    fn the_art_ends_up_in_the_pack() {
        let mut pack = ResourcePack::new("test");
        Art::new(&mut pack);
        assert!(pack.contains("assets/voidmc/font/checkers.json"));
    }

    #[test]
    fn a_ghost_is_see_through() {
        let image = ghost(WHITE, false);
        let centre = image.pixel(9, 9);
        assert!(centre[3] > 0 && centre[3] < 255);
    }
}
