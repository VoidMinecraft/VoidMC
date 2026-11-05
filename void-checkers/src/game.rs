//! Checkers rules (8x8, English draughts), free of any Minecraft type: men move
//! and capture forward, kings move one square in any direction, capturing is
//! compulsory, a capture chain must be finished and promotion ends it.

pub const SIZE: u8 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    White,
    Black,
}

impl Color {
    pub fn opponent(self) -> Self {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }

    /// Row delta of a forward move: white starts at the bottom of the board.
    fn forward(self) -> i8 {
        match self {
            Color::White => -1,
            Color::Black => 1,
        }
    }

    fn promotion_row(self) -> u8 {
        match self {
            Color::White => 0,
            Color::Black => SIZE - 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Piece {
    pub color: Color,
    pub king: bool,
}

/// A playable (dark) square: light squares cannot be represented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Square {
    row: u8,
    col: u8,
}

impl Square {
    pub fn new(row: u8, col: u8) -> Option<Self> {
        (row < SIZE && col < SIZE && (row + col) % 2 == 1).then_some(Self { row, col })
    }

    pub fn from_index(index: u8) -> Option<Self> {
        Self::new(index / SIZE, index % SIZE)
    }

    pub fn index(self) -> u8 {
        self.row * SIZE + self.col
    }

    pub fn row(self) -> u8 {
        self.row
    }

    pub fn col(self) -> u8 {
        self.col
    }

    fn offset(self, rows: i8, cols: i8) -> Option<Self> {
        let row = u8::try_from(self.row as i8 + rows).ok()?;
        let col = u8::try_from(self.col as i8 + cols).ok()?;
        Self::new(row, col)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Move {
    to: Square,
    captured: Option<Square>,
}

/// A piece that changed square.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub from: Square,
    pub to: Square,
    pub captured: Option<Square>,
    pub promoted: bool,
}

/// What a click did to the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ignored,
    Grabbed,
    Released,
    Moved(Step),
}

#[derive(Debug, Clone)]
pub struct Game {
    squares: [Option<Piece>; (SIZE * SIZE) as usize],
    turn: Color,
    selected: Option<Square>,
    /// The selected piece is in the middle of a capture chain and cannot be released.
    chaining: bool,
}

impl Default for Game {
    fn default() -> Self {
        let mut game = Self::empty(Color::White);
        for index in 0..SIZE * SIZE {
            let Some(square) = Square::from_index(index) else {
                continue;
            };
            let color = match square.row {
                0..=2 => Color::Black,
                5..=7 => Color::White,
                _ => continue,
            };
            game.put(square, Some(Piece { color, king: false }));
        }
        game
    }
}

impl Game {
    fn empty(turn: Color) -> Self {
        Self {
            squares: [None; (SIZE * SIZE) as usize],
            turn,
            selected: None,
            chaining: false,
        }
    }

    fn put(&mut self, square: Square, piece: Option<Piece>) {
        self.squares[square.index() as usize] = piece;
    }

    pub fn piece(&self, square: Square) -> Option<Piece> {
        self.squares[square.index() as usize]
    }

    pub fn turn(&self) -> Color {
        self.turn
    }

    pub fn selected(&self) -> Option<Square> {
        self.selected
    }

    /// Whether the selected piece is locked in an unfinished capture chain.
    pub fn chaining(&self) -> bool {
        self.chaining
    }

    /// Whether clicking this square would pick up a piece.
    pub fn can_grab(&self, square: Square) -> bool {
        !self.chaining && self.winner().is_none() && !self.legal_moves(square).is_empty()
    }

    /// The side that won, once the side to move has no legal move left.
    pub fn winner(&self) -> Option<Color> {
        (!self.has_move()).then(|| self.turn.opponent())
    }

    /// Whether the side to move is forced to capture.
    pub fn must_capture(&self) -> bool {
        self.own_squares()
            .any(|square| !self.captures(square).is_empty())
    }

    /// Where the selected piece can go.
    pub fn targets(&self) -> Vec<Square> {
        self.selected
            .map(|square| self.legal_moves(square).iter().map(|m| m.to).collect())
            .unwrap_or_default()
    }

    /// Applies a click: picks up a piece, puts it back when clicked again, or
    /// moves it.
    pub fn click(&mut self, square: Square) -> Outcome {
        if self.winner().is_some() {
            return Outcome::Ignored;
        }
        if let Some(from) = self.selected {
            if from == square && !self.chaining {
                self.selected = None;
                return Outcome::Released;
            }
            if let Some(step) = self.legal_moves(from).into_iter().find(|m| m.to == square) {
                return Outcome::Moved(self.play(from, step));
            }
        }
        if self.chaining || self.legal_moves(square).is_empty() {
            return Outcome::Ignored;
        }
        self.selected = Some(square);
        Outcome::Grabbed
    }

    fn play(&mut self, from: Square, step: Move) -> Step {
        let mut piece = self.piece(from).expect("a legal move starts on a piece");
        let promoted = !piece.king && step.to.row == piece.color.promotion_row();
        piece.king |= promoted;
        self.put(from, None);
        self.put(step.to, Some(piece));
        if let Some(captured) = step.captured {
            self.put(captured, None);
        }
        let chains = step.captured.is_some() && !promoted && !self.captures(step.to).is_empty();
        self.selected = chains.then_some(step.to);
        self.chaining = chains;
        if !chains {
            self.turn = self.turn.opponent();
        }
        Step {
            from,
            to: step.to,
            captured: step.captured,
            promoted,
        }
    }

    fn own_squares(&self) -> impl Iterator<Item = Square> + '_ {
        (0..SIZE * SIZE)
            .filter_map(Square::from_index)
            .filter(|square| self.piece(*square).is_some_and(|p| p.color == self.turn))
    }

    fn has_move(&self) -> bool {
        self.own_squares()
            .any(|square| !self.legal_moves(square).is_empty())
    }

    fn legal_moves(&self, square: Square) -> Vec<Move> {
        if self.piece(square).is_none_or(|p| p.color != self.turn) {
            return Vec::new();
        }
        if self.must_capture() {
            self.captures(square)
        } else {
            self.steps(square)
        }
    }

    fn directions(piece: Piece) -> Vec<(i8, i8)> {
        let forward = piece.color.forward();
        let mut directions = vec![(forward, -1), (forward, 1)];
        if piece.king {
            directions.extend([(-forward, -1), (-forward, 1)]);
        }
        directions
    }

    fn steps(&self, square: Square) -> Vec<Move> {
        let Some(piece) = self.piece(square) else {
            return Vec::new();
        };
        Self::directions(piece)
            .into_iter()
            .filter_map(|(rows, cols)| square.offset(rows, cols))
            .filter(|to| self.piece(*to).is_none())
            .map(|to| Move { to, captured: None })
            .collect()
    }

    fn captures(&self, square: Square) -> Vec<Move> {
        let Some(piece) = self.piece(square) else {
            return Vec::new();
        };
        Self::directions(piece)
            .into_iter()
            .filter_map(|(rows, cols)| {
                let over = square.offset(rows, cols)?;
                let to = square.offset(rows * 2, cols * 2)?;
                let enemy = self.piece(over).is_some_and(|p| p.color != piece.color);
                (enemy && self.piece(to).is_none()).then_some(Move {
                    to,
                    captured: Some(over),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(row: u8, col: u8) -> Square {
        Square::new(row, col).unwrap()
    }

    fn man(color: Color) -> Option<Piece> {
        Some(Piece { color, king: false })
    }

    fn board(turn: Color, pieces: &[(Square, Option<Piece>)]) -> Game {
        let mut game = Game::empty(turn);
        for (square, piece) in pieces {
            game.put(*square, *piece);
        }
        game
    }

    #[test]
    fn light_squares_do_not_exist() {
        assert!(Square::new(0, 0).is_none());
        assert!(Square::new(0, 1).is_some());
        assert!(Square::new(8, 1).is_none());
        assert!(Square::from_index(64).is_none());
    }

    #[test]
    fn the_opening_position_has_twelve_pieces_each_and_white_to_move() {
        let game = Game::default();
        let count = |color| {
            (0..64)
                .filter_map(Square::from_index)
                .filter(|s| game.piece(*s).is_some_and(|p| p.color == color))
                .count()
        };
        assert_eq!(count(Color::White), 12);
        assert_eq!(count(Color::Black), 12);
        assert_eq!(game.turn(), Color::White);
        assert_eq!(game.winner(), None);
    }

    #[test]
    fn clicking_a_piece_selects_it_and_clicking_again_releases_it() {
        let mut game = Game::default();
        assert_eq!(game.click(sq(5, 0)), Outcome::Grabbed);
        assert_eq!(game.selected(), Some(sq(5, 0)));
        assert_eq!(game.targets(), vec![sq(4, 1)]);
        assert_eq!(game.click(sq(5, 0)), Outcome::Released);
        assert_eq!(game.selected(), None);
    }

    #[test]
    fn a_blocked_or_enemy_piece_cannot_be_selected() {
        let mut game = Game::default();
        assert_eq!(game.click(sq(6, 1)), Outcome::Ignored);
        assert_eq!(game.click(sq(2, 1)), Outcome::Ignored);
        assert_eq!(game.selected(), None);
    }

    #[test]
    fn selecting_another_piece_switches_the_selection() {
        let mut game = Game::default();
        game.click(sq(5, 0));
        assert_eq!(game.click(sq(5, 2)), Outcome::Grabbed);
        assert_eq!(game.selected(), Some(sq(5, 2)));
    }

    #[test]
    fn a_move_passes_the_turn() {
        let mut game = Game::default();
        game.click(sq(5, 0));
        assert!(matches!(game.click(sq(4, 1)), Outcome::Moved(_)));
        assert!(game.piece(sq(5, 0)).is_none());
        assert!(game.piece(sq(4, 1)).is_some());
        assert_eq!(game.turn(), Color::Black);
        assert_eq!(game.selected(), None);
    }

    #[test]
    fn men_do_not_move_backwards() {
        let mut game = board(
            Color::White,
            &[(sq(4, 1), man(Color::White)), (sq(0, 1), man(Color::Black))],
        );
        game.click(sq(4, 1));
        assert_eq!(game.targets(), vec![sq(3, 0), sq(3, 2)]);
    }

    #[test]
    fn capturing_is_compulsory() {
        let mut game = board(
            Color::White,
            &[
                (sq(5, 2), man(Color::White)),
                (sq(4, 3), man(Color::Black)),
                (sq(7, 0), man(Color::White)),
            ],
        );
        assert!(game.must_capture());
        assert_eq!(game.click(sq(7, 0)), Outcome::Ignored);
        assert_eq!(game.click(sq(5, 2)), Outcome::Grabbed);
        assert_eq!(game.targets(), vec![sq(3, 4)]);
        let Outcome::Moved(step) = game.click(sq(3, 4)) else {
            panic!("the capture should be played");
        };
        assert_eq!(step.captured, Some(sq(4, 3)));
        assert!(game.piece(sq(4, 3)).is_none());
    }

    #[test]
    fn a_capture_chain_keeps_the_piece_and_the_turn() {
        let mut game = board(
            Color::White,
            &[
                (sq(6, 1), man(Color::White)),
                (sq(5, 2), man(Color::Black)),
                (sq(3, 4), man(Color::Black)),
            ],
        );
        game.click(sq(6, 1));
        game.click(sq(4, 3));
        assert_eq!(game.turn(), Color::White);
        assert_eq!(game.selected(), Some(sq(4, 3)));
        assert!(game.chaining());
        assert!(!game.can_grab(sq(4, 3)));
        assert_eq!(
            game.click(sq(4, 3)),
            Outcome::Ignored,
            "a chaining piece cannot be released"
        );
        assert!(matches!(game.click(sq(2, 5)), Outcome::Moved(_)));
        assert_eq!(game.turn(), Color::Black);
    }

    #[test]
    fn reaching_the_last_row_promotes_and_ends_the_chain() {
        let mut game = board(
            Color::White,
            &[
                (sq(2, 1), man(Color::White)),
                (sq(1, 2), man(Color::Black)),
                (sq(1, 4), man(Color::Black)),
            ],
        );
        game.click(sq(2, 1));
        let Outcome::Moved(step) = game.click(sq(0, 3)) else {
            panic!("the capture should be played");
        };
        assert!(step.promoted);
        assert!(game.piece(sq(0, 3)).unwrap().king);
        assert_eq!(game.turn(), Color::Black);
    }

    #[test]
    fn kings_move_backwards() {
        let king = Some(Piece {
            color: Color::White,
            king: true,
        });
        let mut game = board(
            Color::White,
            &[(sq(3, 2), king), (sq(0, 7), man(Color::Black))],
        );
        game.click(sq(3, 2));
        assert_eq!(game.targets().len(), 4);
    }

    #[test]
    fn a_side_without_moves_loses_and_clicks_stop() {
        let mut game = board(
            Color::Black,
            &[(sq(7, 0), man(Color::Black)), (sq(0, 1), man(Color::White))],
        );
        assert_eq!(game.winner(), Some(Color::White));
        assert_eq!(game.click(sq(7, 0)), Outcome::Ignored);
    }
}
