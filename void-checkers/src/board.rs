//! Draws a game as a dialog: the board between two trays of taken pieces, under
//! the clocks. While a piece is held, every cell shows it as a hover cursor: that
//! is the ghost following the mouse.

use voidmc::TextColor;
use voidmc_ui::{Button, Cell, Dialog, Text, glyph_grid};

use crate::art::Art;
use crate::game::{Color, Game, SIZE, Square, Step};

const NAMESPACE: &str = "voidmc:checkers";
const TICKS_PER_SECOND: u64 = 20;
const TRAY_COLUMNS: u32 = 2;
/// Two trays, two wooden edges, and the board between them.
const COLUMNS: u32 = 2 * TRAY_COLUMNS + 2 + SIZE as u32;
const BOARD_WIDTH: i32 = 240;
const BUTTON_WIDTH: i32 = 120;

/// What a player can ask for by clicking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Click(Square),
    NewGame,
    Close,
}

impl Action {
    pub fn id(self) -> String {
        match self {
            Action::Click(square) => format!("{NAMESPACE}/square/{}", square.index()),
            Action::NewGame => format!("{NAMESPACE}/new"),
            Action::Close => format!("{NAMESPACE}/close"),
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        let path = id.strip_prefix(NAMESPACE)?.strip_prefix('/')?;
        match path {
            "new" => Some(Action::NewGame),
            "close" => Some(Action::Close),
            _ => {
                let index = path.strip_prefix("square/")?.parse().ok()?;
                Square::from_index(index).map(Action::Click)
            }
        }
    }
}

/// A game and what the dialog shows around it.
#[derive(Debug, Clone, Default)]
pub struct Match {
    pub game: Game,
    pub moves: u32,
    pub last: Option<Step>,
    white_ticks: u64,
    black_ticks: u64,
}

impl Match {
    /// Runs the clock of the side to move, until the game ends.
    pub fn tick(&mut self) {
        if self.game.winner().is_some() {
            return;
        }
        match self.game.turn() {
            Color::White => self.white_ticks += 1,
            Color::Black => self.black_ticks += 1,
        }
    }

    fn clock(&self, color: Color) -> String {
        let ticks = match color {
            Color::White => self.white_ticks,
            Color::Black => self.black_ticks,
        };
        let seconds = ticks / TICKS_PER_SECOND;
        format!("{:02}:{:02}", seconds / 60, seconds % 60)
    }
}

fn side(color: Color) -> &'static str {
    match color {
        Color::White => "Blancs",
        Color::Black => "Noirs",
    }
}

fn scoreboard(state: &Match) -> Text {
    let clock = |color| {
        let text = format!("{} {}", side(color), state.clock(color));
        let active = state.game.winner().is_none() && state.game.turn() == color;
        Text::new(text).color(if active {
            TextColor::Yellow
        } else {
            TextColor::Gray
        })
    };
    let moves = Text::new(format!("   Coup {}   ", state.moves + 1)).color(TextColor::White);
    Text::new("")
        .append(clock(Color::White))
        .append(moves)
        .append(clock(Color::Black))
}

fn headline(state: &Match) -> Text {
    let game = &state.game;
    if let Some(winner) = game.winner() {
        return Text::new(format!("{} gagnent !", side(winner))).color(TextColor::Gold);
    }
    if game.chaining() {
        return Text::new("Continue la rafle !").color(TextColor::Red);
    }
    if game.must_capture() {
        return Text::new("Prise obligatoire").color(TextColor::Red);
    }
    match state.last {
        Some(step) => {
            let link = if step.captured.is_some() { "x" } else { "-" };
            let text = format!(
                "Dernier coup : {}{link}{}",
                step.from.name(),
                step.to.name()
            );
            Text::new(text).color(TextColor::Gray)
        }
        None => Text::new("A toi de jouer").color(TextColor::Gray),
    }
}

fn square_cell<'a>(game: &Game, art: &'a Art, targets: &[Square], col: u8, row: u8) -> Cell<'a> {
    let Some(square) = Square::new(row, col) else {
        return Cell::new(&art.light);
    };
    let tile = match game.piece(square) {
        _ if game.selected() == Some(square) => &art.socket,
        Some(piece) => art.piece(piece),
        None if targets.contains(&square) => &art.target,
        None => &art.dark,
    };
    Cell::new(tile).on_click(Action::Click(square).id())
}

/// A slot of a tray. Trays fill up from the bottom.
fn tray_cell<'a>(game: &Game, art: &'a Art, color: Color, column: u32, row: u32) -> Cell<'a> {
    let slot = (u32::from(SIZE) - 1 - row) * TRAY_COLUMNS + column;
    if (slot as usize) < game.captured(color) {
        Cell::new(art.taken(color))
    } else {
        Cell::new(&art.tray)
    }
}

fn cell<'a>(game: &Game, art: &'a Art, targets: &[Square], column: u32, row: u32) -> Cell<'a> {
    let board = TRAY_COLUMNS + 1;
    let right_edge = board + u32::from(SIZE);
    let held = game.selected().and_then(|square| game.piece(square));
    let cell = match column {
        c if c < TRAY_COLUMNS => tray_cell(game, art, Color::Black, c, row),
        c if c < board || c == right_edge => Cell::new(&art.edge),
        c if c < right_edge => square_cell(game, art, targets, (c - board) as u8, row as u8),
        c => tray_cell(game, art, Color::White, c - right_edge - 1, row),
    };
    // Everywhere, trays included: the ghost must not vanish off the board.
    match held {
        Some(piece) => cell.on_hover(art.ghost(piece)),
        None => cell,
    }
}

pub fn dialog(state: &Match, art: &Art) -> Dialog {
    let targets = state.game.targets();
    let board = glyph_grid(&art.font, COLUMNS, u32::from(SIZE), |column, row| {
        cell(&state.game, art, &targets, column, row)
    });
    let new_game = Button::new("Nouvelle partie")
        .width(BUTTON_WIDTH)
        .on_click(Action::NewGame.id());
    let close = Button::new("Fermer")
        .width(BUTTON_WIDTH)
        .on_click(Action::Close.id());
    Dialog::new(Text::new("Dames"), new_game)
        .body(scoreboard(state), BOARD_WIDTH)
        .body(headline(state), BOARD_WIDTH)
        .body(board, BOARD_WIDTH)
        .exit(close)
}

#[cfg(test)]
mod tests {
    use voidmc_ui::ResourcePack;

    use super::*;

    fn art() -> Art {
        Art::new(&mut ResourcePack::new("test"))
    }

    const FIRST_SQUARE: u32 = TRAY_COLUMNS + 1;

    #[test]
    fn action_ids_round_trip() {
        let square = Square::new(5, 0).unwrap();
        for action in [Action::Click(square), Action::NewGame, Action::Close] {
            assert_eq!(Action::parse(&action.id()), Some(action));
        }
    }

    #[test]
    fn foreign_or_invalid_ids_are_ignored() {
        assert_eq!(Action::parse("minecraft:other"), None);
        assert_eq!(Action::parse("voidmc:checkers/square/0"), None);
        assert_eq!(Action::parse("voidmc:checkers/square/99"), None);
    }

    #[test]
    fn light_squares_do_not_react() {
        let art = art();
        let cell = cell(&Game::default(), &art, &[], FIRST_SQUARE, 0);
        assert_eq!(cell, Cell::new(&art.light));
    }

    #[test]
    fn holding_a_piece_turns_it_into_a_hollow_and_a_ghost() {
        let art = art();
        let mut game = Game::default();
        let square = Square::new(5, 0).unwrap();
        game.click(square);
        let piece = game.piece(square).unwrap();
        let expected = Cell::new(&art.socket)
            .on_click(Action::Click(square).id())
            .on_hover(art.ghost(piece));
        assert_eq!(
            cell(&game, &art, &game.targets(), FIRST_SQUARE, 5),
            expected
        );
    }

    #[test]
    fn the_ghost_also_shows_over_the_trays() {
        let art = art();
        let mut game = Game::default();
        let square = Square::new(5, 0).unwrap();
        game.click(square);
        let piece = game.piece(square).unwrap();
        let expected = Cell::new(&art.tray).on_hover(art.ghost(piece));
        assert_eq!(cell(&game, &art, &[], 0, 0), expected);
    }

    #[test]
    fn the_columns_cover_trays_edges_and_board_in_order() {
        let art = art();
        let game = Game::default();
        let last_tray = cell(&game, &art, &[], COLUMNS - 1, 0);
        assert_eq!(last_tray, Cell::new(&art.tray));
        let right_edge = cell(&game, &art, &[], COLUMNS - 1 - TRAY_COLUMNS, 0);
        assert_eq!(right_edge, Cell::new(&art.edge));
    }

    #[test]
    fn only_the_side_to_move_loses_time() {
        let mut state = Match::default();
        for _ in 0..TICKS_PER_SECOND * 65 {
            state.tick();
        }
        assert_eq!(state.clock(Color::White), "01:05");
        assert_eq!(state.clock(Color::Black), "00:00");
    }
}
