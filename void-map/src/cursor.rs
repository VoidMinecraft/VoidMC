//! The pointer in the middle of the screen, drawn by the pack.
//!
//! It is a title. A title is centred on the screen at any size of window, takes real
//! colours, and leaves the client's own crosshair alone, so that nothing changes off
//! the map. That crosshair blends by inverting what is under it, so a sprite of the pack
//! would change colour with the land. It also stays off here: the box around the eye
//! has no menu. A client without the pack keeps it, and a box that has one.
//!
//! The title is also what the veil is made of, and new times or a new text would cut
//! the veil short. So [`Title`] waits for the veil to be gone, and never touches it.

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use voidmc::{EntityKind, Players};
use voidmc_protocol::clientbound::{ClearTitles, SetTitleText, SetTitlesAnimation};
use voidmc_ui::{Image, ResourcePack, Text};

use crate::plugin::MapSession;
use crate::transition::VEIL_FADE;

const FONT: &str = "voidmap:cursor";
const FONT_FILE: &str = "assets/voidmap/font/cursor.json";
const TEXTURE: &str = "voidmap:font/cursor.png";
const TEXTURE_FILE: &str = "assets/voidmap/textures/font/cursor.png";
const FIRST_GLYPH: u32 = 0xE000;

/// The side of a picture, in pixels. One is one pixel of the gui, as in the client's own
/// crosshair, so each stays a whole number of pixels of the screen at any gui scale.
const SIZE: u32 = 16;
/// How much larger the client draws a title than other text.
const TITLE_SCALE: u32 = 4;
/// A title's line starts this far above the middle of the screen, in pixels of its text.
const TITLE_LINE_TOP: i32 = 10;
/// How far a line's baseline is under its top.
const BASELINE: i32 = 7;
/// Longer than a session: nothing renews it. One day.
const STAY: i32 = 20 * 60 * 60 * 24;

const FREE_ROWS: [&str; SIZE as usize] = [
    "................",
    ".......KK.......",
    "......KWWK......",
    "......KWWK......",
    "......KWWK......",
    ".......KK.......",
    ".KKK...KK...KKK.",
    "KWWWK.KWWK.KWWWK",
    "KWWWK.KWWK.KWWWK",
    ".KKK...KK...KKK.",
    ".......KK.......",
    "......KWWK......",
    "......KWWK......",
    "......KWWK......",
    ".......KK.......",
    "................",
];
const OVER_ROWS: [&str; SIZE as usize] = [
    "KKKKKK....KKKKKK",
    "KGGGGK....KGGGGK",
    "KGKKKK....KKKKGK",
    "KGK..........KGK",
    "KGK..........KGK",
    "KKK..........KKK",
    ".......KK.......",
    "......KGGK......",
    "......KGGK......",
    ".......KK.......",
    "KKK..........KKK",
    "KGK..........KGK",
    "KGK..........KGK",
    "KGKKKK....KKKKGK",
    "KGGGGK....KGGGGK",
    "KKKKKK....KKKKKK",
];

/// How the pointer looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// Over nothing.
    Free,
    /// Over a marker, which a click would act on.
    Over,
}

const LOOKS: [Look; 2] = [Look::Free, Look::Over];

impl Look {
    fn glyph(self) -> char {
        let index = LOOKS.iter().position(|look| *look == self);
        char::from_u32(FIRST_GLYPH + index.unwrap_or(0) as u32).expect("a private use character")
    }

    fn rows(self) -> &'static [&'static str; SIZE as usize] {
        match self {
            Self::Free => &FREE_ROWS,
            Self::Over => &OVER_ROWS,
        }
    }

    fn text(self) -> Text {
        Text::new(self.glyph()).font(FONT).no_shadow()
    }
}

/// What the title of a player in the map has to be, this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wanted {
    /// The veil has it.
    Veil,
    /// Nothing: the view is busy, or the client has no pack.
    Nothing,
    Pointer(Look),
}

/// The box around the eye. The client only shows a spectator its crosshair over
/// something with a menu, and any box takes clicks.
pub fn eye_box_kind(drawn: bool) -> EntityKind {
    if drawn {
        EntityKind::Minecart
    } else {
        EntityKind::ChestMinecart
    }
}

/// What this server left in a player's title.
#[derive(Component, Debug, Default, PartialEq, Eq)]
struct Title {
    pointer: Option<Look>,
    /// The ticks left before the veil is gone.
    veil_going: u32,
}

/// What has to change on the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    Start(Look),
    Swap(Look),
    Clear,
}

impl Title {
    /// Moves on by a tick.
    fn tick(&mut self, wanted: Wanted) -> Option<Change> {
        if wanted == Wanted::Veil {
            self.pointer = None;
            self.veil_going = VEIL_FADE;
            return None;
        }
        if self.veil_going > 0 {
            self.veil_going -= 1;
            return None;
        }
        let change = match (wanted, self.pointer) {
            (Wanted::Pointer(look), None) => Change::Start(look),
            (Wanted::Pointer(look), Some(shown)) if look != shown => Change::Swap(look),
            (Wanted::Nothing, Some(_)) => Change::Clear,
            _ => return None,
        };
        self.pointer = match wanted {
            Wanted::Pointer(look) => Some(look),
            _ => None,
        };
        Some(change)
    }
}

impl Change {
    fn send(self, players: &Players, player: Entity) {
        match self {
            // With no fade, so that a change of look is at once.
            Self::Start(look) => {
                let times = SetTitlesAnimation {
                    fade_in: 0,
                    stay: STAY,
                    fade_out: 0,
                };
                players.send(player, times);
                players.send(player, title_text(look));
            }
            Self::Swap(look) => players.send(player, title_text(look)),
            Self::Clear => players.send(player, ClearTitles { reset_times: false }),
        }
    }
}

fn title_text(look: Look) -> SetTitleText {
    SetTitleText {
        text: look.text().to_nbt(),
    }
}

fn show(
    sessions: Query<(Entity, &MapSession, Option<&mut Title>)>,
    players: Players,
    mut commands: Commands,
) {
    for (player, session, title) in sessions {
        let Some(mut title) = title else {
            commands.entity(player).insert(Title::default());
            continue;
        };
        if let Some(change) = title.tick(session.wanted_cursor()) {
            change.send(&players, player);
        }
    }
}

pub struct CursorPlugin;

impl Plugin for CursorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, show);
    }
}

fn texture() -> Image {
    let mut image = Image::new(SIZE * LOOKS.len() as u32, SIZE);
    for (index, look) in LOOKS.iter().enumerate() {
        for (y, row) in look.rows().iter().enumerate() {
            for (x, pixel) in row.bytes().enumerate() {
                let color = match pixel {
                    b'W' => [255, 255, 255, 255],
                    b'G' => [255, 196, 48, 255],
                    b'K' => [16, 18, 24, 255],
                    _ => continue,
                };
                image.set(index as u32 * SIZE + x as u32, y as u32, color);
            }
        }
    }
    image
}

/// So that the picture sits around the middle of the screen, half above and half under.
fn font() -> String {
    let height = (SIZE / TITLE_SCALE) as i32;
    let ascent = BASELINE - TITLE_LINE_TOP + height / 2;
    let glyphs: String = LOOKS
        .iter()
        .map(|look| format!("\\u{:04X}", look.glyph() as u32))
        .collect();
    format!(
        r#"{{"providers":[{{"type":"bitmap","file":"{TEXTURE}","height":{height},"ascent":{ascent},"chars":["{glyphs}"]}}]}}"#
    )
}

/// Adds the pictures, in a font of their own.
pub fn install(pack: &mut ResourcePack) {
    pack.add(TEXTURE_FILE, texture().to_png());
    pack.add(FONT_FILE, font().into_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    const FREE: Wanted = Wanted::Pointer(Look::Free);
    const OVER: Wanted = Wanted::Pointer(Look::Over);

    #[test]
    fn a_pointer_starts_once_then_only_swaps() {
        let mut title = Title::default();
        assert_eq!(title.tick(FREE), Some(Change::Start(Look::Free)));
        assert_eq!(title.tick(FREE), None);
        assert_eq!(title.tick(OVER), Some(Change::Swap(Look::Over)));
    }

    #[test]
    fn the_veil_is_left_alone_until_it_is_gone() {
        let mut title = Title::default();
        title.tick(FREE);
        assert_eq!(title.tick(Wanted::Veil), None);
        for _ in 0..VEIL_FADE {
            assert_eq!(title.tick(FREE), None);
        }
        assert_eq!(title.tick(FREE), Some(Change::Start(Look::Free)));
    }

    #[test]
    fn a_busy_view_clears_the_pointer_once() {
        let mut title = Title::default();
        title.tick(OVER);
        assert_eq!(title.tick(Wanted::Nothing), Some(Change::Clear));
        assert_eq!(title.tick(Wanted::Nothing), None);
        assert_eq!(title.tick(FREE), Some(Change::Start(Look::Free)));
    }

    #[test]
    fn a_client_with_no_pointer_never_hears_of_it() {
        assert_eq!(Title::default().tick(Wanted::Nothing), None);
    }

    /// The client centres a title by its advance, which comes from the last pixel drawn.
    #[test]
    fn each_picture_is_exactly_as_wide_as_the_title_expects() {
        for look in LOOKS {
            assert!(look.rows().iter().all(|row| row.len() == SIZE as usize));
            assert!(look.rows().iter().any(|row| row.ends_with(['K', 'W', 'G'])));
        }
    }

    /// Half of a 16 pixel picture over the middle, at four times the size of text.
    #[test]
    fn the_font_puts_the_picture_around_the_middle() {
        assert!(font().contains(r#""height":4,"ascent":-1"#));
    }

    #[test]
    fn only_a_box_for_a_client_with_no_pack_shows_the_crosshair() {
        assert_eq!(eye_box_kind(true), EntityKind::Minecart);
        assert_eq!(eye_box_kind(false), EntityKind::ChestMinecart);
    }

    #[test]
    fn the_pack_holds_the_font_and_its_picture() {
        let mut pack = ResourcePack::new("test");
        install(&mut pack);
        assert!(pack.contains(FONT_FILE) && pack.contains(TEXTURE_FILE));
    }
}
