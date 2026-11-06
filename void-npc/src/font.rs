//! Where text goes on the screen. The client lays out a text from one point, so everything
//! placed elsewhere rides on two things of the pack: characters that only move the pen, and
//! copies of the vanilla font that sit lower by a few pixels. A bitmap can go down as far as
//! it likes, but its ascent cannot pass its height: a font can only lower text, never raise it.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde_json::{Value, json};
use voidmc_ui::ResourcePack;

pub const NAMESPACE: &str = "voidnpc";
/// Text can start every this many pixels down.
pub const ROW_STEP: i32 = 3;
/// The lowest a text can go, in pixels.
pub const ROW_REACH: i32 = 240;
const FIRST_MOVE: u32 = 0xE000;
const MOVE_BITS: u32 = 12;
const SPACE_WIDTH: i32 = 4;
/// What a character with no known width counts for.
const UNKNOWN_WIDTH: i32 = 6;

struct Vanilla {
    providers: Vec<(String, i32, i32, Value)>,
    widths: BTreeMap<char, i32>,
}

/// Read from the jar of 26.1.2 by hand: which characters each picture of the font holds, and
/// how far the pen moves over each.
static VANILLA: LazyLock<Vanilla> = LazyLock::new(|| {
    let data: Value =
        serde_json::from_str(include_str!("vanilla_font.json")).expect("the table is valid");
    let provider = |name: &str| {
        let entry = &data[name];
        (
            format!("minecraft:font/{name}.png"),
            entry["ascent"].as_i64().expect("an ascent") as i32,
            entry["height"].as_i64().expect("a height") as i32,
            entry["chars"].clone(),
        )
    };
    let widths = data["widths"].as_object().expect("a table of widths");
    let widths = widths
        .iter()
        .filter_map(|(character, width)| Some((character.chars().next()?, width.as_i64()? as i32)));
    Vanilla {
        providers: vec![
            provider("nonlatin_european"),
            provider("accented"),
            provider("ascii"),
        ],
        widths: widths.collect(),
    }
});

/// How far the pen moves over `text` in the vanilla font.
pub fn text_width(text: &str) -> i32 {
    text.chars()
        .map(|character| match character {
            ' ' => SPACE_WIDTH,
            other => VANILLA.widths.get(&other).copied().unwrap_or(UNKNOWN_WIDTH),
        })
        .sum()
}

/// `text` cut into lines no wider than `width`, at its spaces.
pub fn wrap(text: &str, width: i32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let longer = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
        if text_width(&longer) > width && !line.is_empty() {
            lines.push(std::mem::replace(&mut line, word.to_string()));
        } else {
            line = longer;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

fn move_glyph(bit: u32, back: bool) -> char {
    let at = FIRST_MOVE + bit * 2 + u32::from(back);
    char::from_u32(at).expect("private-use characters are valid")
}

/// Characters that move the pen by `pixels`, in any font of this module.
pub fn moved(pixels: i32) -> String {
    let distance = pixels.unsigned_abs();
    assert!(distance < 1 << MOVE_BITS, "a move of {pixels} is too far");
    (0..MOVE_BITS)
        .filter(|bit| distance & (1 << bit) != 0)
        .map(|bit| move_glyph(bit, pixels < 0))
        .collect()
}

/// The moves, and the space the vanilla font gets from a provider of its own.
pub(crate) fn move_provider() -> Value {
    let mut advances = serde_json::Map::new();
    advances.insert(" ".to_string(), json!(SPACE_WIDTH));
    for bit in 0..MOVE_BITS {
        let distance = 1 << bit;
        advances.insert(move_glyph(bit, false).to_string(), json!(distance));
        advances.insert(move_glyph(bit, true).to_string(), json!(-distance));
    }
    json!({ "type": "space", "advances": advances })
}

/// The font that draws text `y` pixels down, rounded to [`ROW_STEP`].
pub fn row_font(y: i32) -> String {
    format!("{NAMESPACE}:row_{}", row_of(y))
}

fn row_of(y: i32) -> i32 {
    let row = (y + ROW_STEP / 2).div_euclid(ROW_STEP);
    assert!(
        (0..=ROW_REACH / ROW_STEP).contains(&row),
        "a font can only lower text, down to {ROW_REACH} pixels, not to {y}"
    );
    row
}

/// Writes every row into the pack.
pub(crate) fn install(pack: &mut ResourcePack) {
    for row in 0..=ROW_REACH / ROW_STEP {
        let mut providers = vec![move_provider()];
        for (file, ascent, height, chars) in &VANILLA.providers {
            providers.push(json!({
                "type": "bitmap",
                "file": file,
                "ascent": ascent - row * ROW_STEP,
                "height": height,
                "chars": chars,
            }));
        }
        providers.push(json!({ "type": "reference", "id": "minecraft:default" }));
        pack.add(
            format!("assets/{NAMESPACE}/font/row_{row}.json"),
            json!({ "providers": providers }).to_string().into_bytes(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_punctuation_of_french_sits_on_the_row() {
        let known = |text: &str| text.chars().all(|c| VANILLA.widths.contains_key(&c));
        assert!(known("’…—«»•×œ"));
    }

    #[test]
    fn widths_are_those_of_the_jar() {
        assert_eq!(text_width("i"), 2);
        assert_eq!(text_width("a b"), 16);
        assert_eq!(text_width("é"), 6);
    }

    #[test]
    fn a_line_never_passes_its_width() {
        let lines = wrap(
            "Un marchand ne dit jamais tout ce qu'il sait, voyageur.",
            100,
        );
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|line| text_width(line) <= 100));
    }

    #[test]
    fn nothing_is_lost_when_wrapping() {
        let text = "un deux trois quatre";
        assert_eq!(wrap(text, 30).join(" "), text);
    }

    #[test]
    fn a_word_too_long_stays_whole() {
        assert_eq!(wrap("anticonstitutionnellement", 10).len(), 1);
    }

    #[test]
    fn a_move_adds_up_to_its_distance() {
        let distance = |text: String| -> i32 {
            text.chars()
                .map(|glyph| {
                    let at = glyph as u32 - FIRST_MOVE;
                    let pixels = 1 << (at / 2);
                    if at % 2 == 1 { -pixels } else { pixels }
                })
                .sum()
        };
        for pixels in [1, -1, 37, -999, 2047] {
            assert_eq!(distance(moved(pixels)), pixels);
        }
    }

    #[test]
    fn nothing_moves_for_no_distance() {
        assert!(moved(0).is_empty());
    }

    #[test]
    fn rows_round_to_the_nearest_step() {
        assert_eq!(row_font(0), "voidnpc:row_0");
        assert_eq!(row_font(4), "voidnpc:row_1");
        assert_eq!(row_font(5), "voidnpc:row_2");
    }

    #[test]
    #[should_panic(expected = "only lower")]
    fn no_font_raises_text() {
        row_font(-9);
    }

    #[test]
    fn every_row_lands_in_the_pack() {
        let mut pack = ResourcePack::new("test");
        install(&mut pack);
        assert!(pack.contains("assets/voidnpc/font/row_0.json"));
        assert!(pack.contains(&format!(
            "assets/voidnpc/font/row_{}.json",
            ROW_REACH / ROW_STEP
        )));
    }
}
