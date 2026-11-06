//! What the player sees of a scene. Speech goes through the action bar, which the client
//! draws under a screen and redraws without touching it: a line can come letter by letter
//! while the dialog over it stays still, the pointer and what it hovers with it.
//!
//! A dialog gives no say on where its buttons go. It centres a grid of them, so a wide cell
//! in a first column pushes the others to the right. That cell would show, so it goes in a
//! last row, below the bottom of the screen, where the scroll area of the dialog cuts it.

use voidmc::TextColor;
use voidmc_protocol::clientbound::ClientboundPacket;
use voidmc_protocol::clientbound::{SetTitleText, SetTitlesAnimation, SystemChat};
use voidmc_ui::{Button, Dialog, Text};

use crate::art::{ART_FONT, Art, DIALOG_TITLE_TOP};
use crate::canvas::Canvas;
use crate::conversation::Choice;
use crate::font::{moved, text_width};
use crate::interface::{Control, Grid, Screen};
use crate::npc::Theme;

pub(crate) const NEXT: &str = "voidnpc:next";
pub(crate) const LEAVE: &str = "voidnpc:leave";
const CHOICE: &str = "voidnpc:choice/";
const INTERFACE: &str = "voidnpc:do/";

pub const RULE_Y: i32 = 11;
const SPEECH_Y: i32 = 21;
const SPEECH_STEP: i32 = 9;
const HINT_Y: i32 = 48;
const HINT: &str = "Clic pour continuer";
/// How long a full pulse of the hint takes, in ticks.
const HINT_PULSE: u32 = 30;
const HINT_DIM: u32 = 0x5a;
const HINT_BRIGHT: u32 = 0xd0;
/// Speech makes room for the choices, on its left.
const SPEECH_SHIFT: i32 = -84;
/// Half of the gap of 10 and the button of 20 beside a title, and of the space ending it.
const TITLE_OFF_CENTRE: i32 = 17;

pub const CHOICE_WIDTH: i32 = 136;
/// As far right as a window of 1280 by 720 allows at its usual scale: the grid is placed from
/// the middle of the screen, and a client that cannot fit it moves it all left.
const CHOICES_X: i32 = 56;
const CHOICES_TOP: i32 = 115;
const CHOICE_MARK_X: i32 = 6;
const CHOICE_TEXT_X: i32 = 17;
const FILLER_WIDTH: i32 = 1;
/// A full column of these is enough to reach the bottom of any screen.
const TAIL_ROWS: usize = 50;
const GRID_GAP: i32 = 2;
/// Where the scroll area of a dialog starts, and how far its text blocks stand apart.
const CONTENT_TOP: i32 = 33;
const BODY_GAP: i32 = 10;
const LINE_HEIGHT: i32 = 9;

const CLICK_ROWS: usize = 64;
/// How wide a text block can be, and how much it keeps of that for its padding and the pin
/// at each end of a line: a line that does not fit is cut, and takes no clicks.
const BODY_WIDTH: i32 = 1024;
const CLICK_WIDTH: i32 = BODY_WIDTH - 20;

const VEIL_TICKS: u32 = 5;

/// A line of speech, as far as it has been typed.
pub struct Speech<'a> {
    pub name: &'a str,
    pub lines: &'a [String],
    pub typed: usize,
    pub hint: Option<u32>,
    pub aside: bool,
}

fn bars(canvas: &mut Canvas, art: &Art) {
    canvas.picture(
        -art.bottom_bar.width() / 2,
        &art.bottom_bar,
        TextColor::White,
    );
}

/// The action bar of a scene with nothing said.
pub fn silence(art: &Art) -> SystemChat {
    let mut canvas = Canvas::new();
    bars(&mut canvas, art);
    overlay(canvas.finish())
}

/// Speech under the NPC's name.
pub fn speech(art: &Art, theme: &Theme, speech: &Speech) -> SystemChat {
    let mut canvas = Canvas::new();
    bars(&mut canvas, art);
    let middle = if speech.aside { SPEECH_SHIFT } else { 0 };
    canvas.centered(middle, 0, speech.name, TextColor::Rgb(theme.accent));
    canvas.picture(
        middle - art.rule.width() / 2,
        &art.rule,
        TextColor::Rgb(theme.accent),
    );
    let mut left = speech.typed;
    for (index, line) in speech.lines.iter().enumerate() {
        let shown: String = line.chars().take(left).collect();
        left -= shown.chars().count().min(left);
        if !shown.is_empty() {
            let x = middle - text_width(line) / 2;
            let y = SPEECH_Y + index as i32 * SPEECH_STEP;
            canvas.text(x, y, &shown, TextColor::White);
        }
    }
    if let Some(tick) = speech.hint {
        canvas.centered(middle, HINT_Y, HINT, TextColor::Rgb(pulse(tick)));
    }
    overlay(canvas.finish())
}

fn overlay(text: Text) -> SystemChat {
    SystemChat {
        content: text.to_nbt(),
        overlay: true,
    }
}

fn pulse(tick: u32) -> u32 {
    let half = HINT_PULSE / 2;
    let phase = tick % HINT_PULSE;
    let rising = if phase < half {
        phase
    } else {
        HINT_PULSE - phase
    };
    let grey = HINT_DIM + (HINT_BRIGHT - HINT_DIM) * rising / half;
    grey << 16 | grey << 8 | grey
}

/// What the action bar says to someone looking at an NPC.
pub fn prompt() -> SystemChat {
    let key = Text::keybind("key.use").color(TextColor::Aqua);
    overlay(
        Text::new("[")
            .color(TextColor::Aqua)
            .append(key)
            .append(Text::new("] Parler")),
    )
}

pub fn no_prompt() -> SystemChat {
    overlay(Text::new(""))
}

/// The bar over the scene, then `canvas`, as the title of a dialog: the first thing a dialog
/// draws, wherever it sits, and under all the rest. A title of no width would be dropped.
/// The client centres a title together with the warning button at its side, so the middle
/// of the title is [`TITLE_OFF_CENTRE`] left of the middle of the screen.
fn title(art: &Art, canvas: Canvas) -> Text {
    let mut bar = Canvas::new();
    bar.picture(-art.top_bar.width() / 2, &art.top_bar, TextColor::White);
    let moving = |pixels: i32| Text::new(moved(pixels)).font(ART_FONT).no_shadow();
    bar.finish()
        .append(moving(TITLE_OFF_CENTRE))
        .append(canvas.finish())
        .append(moving(-TITLE_OFF_CENTRE))
        .append(Text::new(" "))
}

fn exit() -> Button {
    Button::new("").width(FILLER_WIDTH).on_click(LEAVE)
}

fn filler() -> Button {
    Button::new("").width(FILLER_WIDTH)
}

/// A dialog that takes a click anywhere. The client only counts text that has a bounding box,
/// which pure moves do not give it: a nearly clear pin at each end makes one.
pub fn listening(art: &Art) -> Dialog {
    let pin = art.pin.glyph();
    let row = format!("{pin}{}{pin}", moved(CLICK_WIDTH));
    let rows = (0..CLICK_ROWS).flat_map(|at| {
        let line = Text::new(row.clone())
            .font(ART_FONT)
            .no_shadow()
            .on_click(NEXT);
        let start = (at > 0).then(|| Text::new("\n"));
        start.into_iter().chain(std::iter::once(line))
    });
    Dialog::new(title(art, Canvas::new()), filler())
        .body(Text::new("").extend(rows), BODY_WIDTH)
        .exit(exit())
}

/// `choices` by their place among all the answers of a node, on the right of the screen.
pub fn choosing(art: &Art, theme: &Theme, choices: &[(usize, &Choice)]) -> Dialog {
    let rows = choices.iter().map(|(index, choice)| {
        let label = choice_label(art, theme, &choice.label);
        vec![Some(Control::new(label, format!("{CHOICE}{index}")))]
    });
    let grid = Grid {
        x: CHOICES_X,
        top: CHOICES_TOP,
        widths: vec![CHOICE_WIDTH],
        rows: rows.collect(),
    };
    grid_dialog(art, Canvas::new(), &grid)
}

fn choice_label(art: &Art, theme: &Theme, label: &str) -> Text {
    let left = -CHOICE_WIDTH / 2;
    let mut canvas = Canvas::new();
    canvas.picture(left + 1, &art.fill, TextColor::Rgb(theme.accent));
    canvas.picture(
        left + CHOICE_MARK_X,
        &art.mark,
        TextColor::Rgb(theme.accent),
    );
    canvas.text(left + CHOICE_TEXT_X, 0, label, TextColor::White);
    canvas.finish()
}

/// The screen of an interface.
pub fn interface(art: &Art, screen: Screen) -> Dialog {
    grid_dialog(art, screen.canvas, &screen.grid)
}

/// The width of the cell that puts the first column at `grid.x` from the middle.
fn spacer_width(grid: &Grid) -> i32 {
    let gaps = GRID_GAP * (grid.widths.len() as i32 - 1);
    let columns = grid.widths.iter().sum::<i32>() + gaps;
    (2 * grid.x + columns - GRID_GAP).max(FILLER_WIDTH)
}

fn grid_dialog(art: &Art, canvas: Canvas, grid: &Grid) -> Dialog {
    let columns = grid.widths.len();
    let mut cells = Vec::new();
    for row in &grid.rows {
        cells.push(filler());
        for (at, width) in grid.widths.iter().enumerate() {
            cells.push(match row.get(at).cloned().flatten() {
                Some(control) => control.button(*width),
                None => filler(),
            });
        }
    }
    cells.extend(std::iter::repeat_n(filler(), TAIL_ROWS * (columns + 1)));
    cells.push(Button::new("").width(spacer_width(grid)));
    cells.extend(std::iter::repeat_n(filler(), columns));
    let mut cells = cells.into_iter();
    let first = cells.next().unwrap_or_else(filler);
    let mut dialog = cells
        .fold(Dialog::new(title(art, canvas), first), Dialog::button)
        .columns(columns as i32 + 1)
        .exit(exit());
    if let Some(blank) = blank_lines(grid.top) {
        dialog = dialog.body(blank, 1);
    }
    dialog
}

/// Text that pushes a grid down to about `top`. With none, it starts at [`CONTENT_TOP`].
fn blank_lines(top: i32) -> Option<Text> {
    let lines = (top - CONTENT_TOP - BODY_GAP) / LINE_HEIGHT;
    (lines > 0).then(|| Text::new(vec![" "; lines as usize].join("\n")))
}

/// Where a grid that asked for `top` starts on the screen.
pub fn grid_top(top: i32) -> i32 {
    match (top - CONTENT_TOP - BODY_GAP) / LINE_HEIGHT {
        lines if lines > 0 => CONTENT_TOP + BODY_GAP + lines * LINE_HEIGHT,
        _ => CONTENT_TOP,
    }
}

/// A y on the screen, on a canvas that goes in a dialog title.
pub fn from_top(y: i32) -> i32 {
    y - DIALOG_TITLE_TOP
}

/// Brings the veil over the screen. It stays until [`veil_out`].
pub fn veil_in(art: &Art) -> [ClientboundPacket; 2] {
    [
        SetTitlesAnimation {
            fade_in: VEIL_TICKS as i32,
            stay: 400,
            fade_out: VEIL_TICKS as i32,
        }
        .into(),
        SetTitleText {
            text: Text::new(art.veil.glyph().to_string())
                .font(ART_FONT)
                .no_shadow()
                .to_nbt(),
        }
        .into(),
    ]
}

/// New times restart the clock of the title, here on its last stretch.
pub fn veil_out() -> ClientboundPacket {
    SetTitlesAnimation {
        fade_in: 0,
        stay: 0,
        fade_out: VEIL_TICKS as i32,
    }
    .into()
}

pub const VEIL_FULL_TICKS: u32 = VEIL_TICKS + 1;

/// What a click of a player is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click<'a> {
    Next,
    Leave,
    Choice(usize),
    Interface(&'a str),
}

impl<'a> Click<'a> {
    pub fn parse(id: &'a str) -> Option<Self> {
        match id {
            NEXT => Some(Self::Next),
            LEAVE => Some(Self::Leave),
            _ => id
                .strip_prefix(CHOICE)
                .and_then(|index| Some(Self::Choice(index.parse().ok()?)))
                .or_else(|| id.strip_prefix(INTERFACE).map(Self::Interface)),
        }
    }
}

/// The id of a click on a control of an interface.
pub(crate) fn interface_click(action: &str) -> String {
    format!("{INTERFACE}{action}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_survives_its_id() {
        assert_eq!(Click::parse(NEXT), Some(Click::Next));
        assert_eq!(Click::parse(LEAVE), Some(Click::Leave));
        assert_eq!(Click::parse("voidnpc:choice/3"), Some(Click::Choice(3)));
        let id = interface_click("buy/2");
        assert_eq!(Click::parse(&id), Some(Click::Interface("buy/2")));
    }

    #[test]
    fn the_clicks_of_others_are_left_alone() {
        for id in ["voidmc:checkers/new", "voidnpc:choice/x", "voidnpc:other"] {
            assert_eq!(Click::parse(id), None);
        }
    }

    #[test]
    fn the_spacer_puts_the_first_column_where_asked() {
        let grid = Grid {
            x: CHOICES_X,
            top: 0,
            widths: vec![136],
            rows: Vec::new(),
        };
        let spacer = spacer_width(&grid);
        let whole = spacer + GRID_GAP + 136;
        assert_eq!(-whole / 2 + spacer + GRID_GAP, CHOICES_X);
    }

    #[test]
    fn a_grid_starts_on_a_line_of_the_body() {
        assert_eq!(grid_top(0), CONTENT_TOP);
        assert_eq!(grid_top(115), 115);
        assert_eq!(grid_top(117), 115);
    }

    #[test]
    fn the_hint_stays_within_its_greys() {
        for tick in 0..HINT_PULSE * 2 {
            let grey = pulse(tick) & 0xff;
            assert!((HINT_DIM..=HINT_BRIGHT).contains(&grey));
        }
    }
}
