//! A drawing on one line of text. The dialog title and the action bar each centre what they
//! are given, so a canvas ends where it started: its width is nothing and `x = 0` is the
//! middle of the screen. The client trims icons, which leaves a canvas with icons a few pixels
//! off the middle.

use voidmc::TextColor;
use voidmc_ui::Text;

use crate::art::{ART_FONT, Icon, Picture};
use crate::font::{moved, row_font, text_width};

/// Places text and pictures from a point in the middle of the screen, `y` going down.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Canvas {
    parts: Vec<Text>,
    pen: i32,
}

impl Canvas {
    pub fn new() -> Self {
        Self::default()
    }

    fn go(&mut self, x: i32, font: &str) {
        if x != self.pen {
            self.parts
                .push(Text::new(moved(x - self.pen)).font(font).no_shadow());
            self.pen = x;
        }
    }

    /// Draws `text` from `x`, `y`, with its shadow. `y` rounds to the nearest row.
    pub fn text(&mut self, x: i32, y: i32, text: &str, color: TextColor) -> &mut Self {
        let font = row_font(y);
        self.go(x, &font);
        self.parts.push(Text::new(text).font(font).color(color));
        self.pen += text_width(text);
        self
    }

    /// Draws `text` so that its middle falls on `x`.
    pub fn centered(&mut self, x: i32, y: i32, text: &str, color: TextColor) -> &mut Self {
        self.text(x - text_width(text) / 2, y, text, color)
    }

    /// Draws `picture` from `x`, tinted by `color` where it is white.
    pub fn picture(&mut self, x: i32, picture: &Picture, color: TextColor) -> &mut Self {
        self.go(x, ART_FONT);
        self.parts.push(
            Text::new(picture.glyph().to_string())
                .font(ART_FONT)
                .color(color)
                .no_shadow(),
        );
        self.pen += picture.advance();
        self
    }

    /// Draws an icon from `x`. What the client trims off its sides shifts all that comes
    /// after it, so icons go last.
    pub fn icon(&mut self, x: i32, icon: &Icon) -> &mut Self {
        self.go(x, ART_FONT);
        self.parts.push(
            Text::new(icon.glyph().to_string())
                .font(ART_FONT)
                .color(TextColor::White)
                .no_shadow(),
        );
        self.pen += icon.advance();
        self
    }

    /// Everything drawn, with the pen back at the start.
    pub fn finish(mut self) -> Text {
        self.go(0, ART_FONT);
        Text::new("").extend(self.parts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::{moved, text_width};

    /// What the pen did, from the text the canvas gives the client.
    fn total_move(canvas: Canvas) -> String {
        format!("{:?}", canvas.finish())
    }

    #[test]
    fn a_canvas_with_nothing_is_empty() {
        assert_eq!(Canvas::new().finish(), Text::new(""));
    }

    #[test]
    fn the_pen_comes_back_after_text() {
        let mut canvas = Canvas::new();
        canvas.text(10, 0, "abc", TextColor::White);
        let shown = total_move(canvas);
        assert!(shown.contains(&format!("{:?}", moved(10))));
        assert!(shown.contains(&format!("{:?}", moved(-10 - text_width("abc")))));
    }

    #[test]
    fn centred_text_starts_half_its_width_to_the_left() {
        let mut canvas = Canvas::new();
        canvas.centered(0, 0, "abcd", TextColor::White);
        assert_eq!(canvas.pen, text_width("abcd") - text_width("abcd") / 2);
    }
}
