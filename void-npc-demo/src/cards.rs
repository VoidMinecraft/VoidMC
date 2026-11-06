//! A fortune teller's table: three cards face down, and what the one a player turns says.
//! Nothing here comes from the crate but [`Interface`]: this is how an NPC gets a screen of
//! its own.

use std::f64::consts::PI;
use std::time::{SystemTime, UNIX_EPOCH};

use voidmc::{Particle, Sound, TextColor};
use voidmc_npc::art::ArtBuilder;
use voidmc_npc::interface::Grid;
use voidmc_npc::view::from_top;
use voidmc_npc::{
    Canvas, Control, Cx, Gesture, Interface, Placing, Prop, Reply, Screen, Shot, Theme,
};
use voidmc_ui::{Image, Text};

const CARD: (u32, u32) = (44, 66);
/// In sixteenths of a block.
const CARD_SIZE: (f64, f64) = (11.0, 16.5);
const COLUMN_WIDTH: i32 = 52;
const GAP: i32 = 2;
const TURN_TOP: i32 = 178;
const THANKS_TOP: i32 = 214;
const HEADING_Y: i32 = 94;
const FATE_Y: i32 = 183;
const SAYING_Y: i32 = 195;
const SAYING_STEP: i32 = 9;
const DEAL_SOUND: &str = "item.book.page_turn";
const TURN_SOUND: &str = "block.enchantment_table.use";
const REVEAL_SOUND: &str = "block.amethyst_block.resonate";

/// Where the cards float: between the view of [`Shot::Face`] and the chest of the NPC.
const SLOT_NEAR: f64 = 0.45;
const SLOT_UP: f64 = 1.3;
const SLOT_MIDDLE: f64 = 0.045;
const SLOT_STEP: f64 = 0.29;
const CARD_SCALE: f64 = 0.35;
/// A keyframe every this many ticks, which the client fills in. A turn takes the short way
/// between two keyframes, so a card spins less than half a lap in that time.
const STEP: u32 = 2;
const CALM_STEP: u32 = 5;
const DEAL_START: u32 = 8;
const DEAL_TICKS: u32 = 22;
const DEAL_LAPS: f64 = 2.0;
const DEALT_AT: u32 = DEAL_START * 2 + DEAL_TICKS + 2;
const TURN_TICKS: u32 = 40;
const TURN_LAPS: f64 = 3.0;
const SPARK_STEP: u32 = 3;

const FATES: [(&str, [&str; 2]); 3] = [
    ("La Route", ["Ton chemin est long,", "mais il est à toi."]),
    (
        "La Lanterne",
        ["Une lumière t'attend", "là où tu ne regardes pas."],
    ),
    (
        "Le Corbeau",
        ["Méfie-toi de qui parle", "plus qu'il n'écoute."],
    ),
];

fn frame(body: [u8; 4], line: [u8; 4]) -> Image {
    let (width, height) = CARD;
    let mut image = Image::filled(width, height, body);
    for x in 0..width {
        for y in 0..height {
            let edge = x.min(y).min(width - 1 - x).min(height - 1 - y);
            if edge == 2 {
                image.set(x, y, line);
            }
        }
    }
    image
}

fn back() -> Image {
    let (width, height) = CARD;
    let mut image = frame([40, 26, 74, 255], [198, 150, 240, 255]);
    let (cx, cy) = (width as i32 / 2, height as i32 / 2);
    for x in 0..width as i32 {
        for y in 0..height as i32 {
            let distance = (x - cx).abs() + (y - cy).abs();
            if distance == 12 || distance == 6 {
                image.set(x as u32, y as u32, [198, 150, 240, 255]);
            }
        }
    }
    image
}

fn face(symbol: usize) -> Image {
    let (width, height) = CARD;
    let mut image = frame([232, 222, 200, 255], [120, 80, 170, 255]);
    let ink = [70, 40, 110, 255];
    let (cx, cy) = (width as i32 / 2, height as i32 / 2);
    for x in 0..width as i32 {
        for y in 0..height as i32 {
            let (dx, dy) = (x - cx, y - cy);
            let on = match symbol {
                0 => dx.abs() == (dy + 22).abs() / 4 + 2 && dy.abs() < 20,
                1 => dx * dx + dy * dy < 90 || (dx.abs() < 2 && dy.abs() < 20),
                _ => dy.abs() < 3 + dx.abs() / 3 && dx.abs() < 14 && (dx + dy) % 3 != 0,
            };
            if on {
                image.set(x as u32, y as u32, ink);
            }
        }
    }
    image
}

/// Three cards dealt from the hands of the NPC, one to turn.
#[derive(Debug, Clone, Default)]
pub struct Cards {
    props: Vec<Prop>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Moment {
    #[default]
    Dealing,
    Waiting,
    Turning(usize),
    Shown(usize),
}

/// How a draw is going. `fates` says what lies under each place.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Table {
    moment: Moment,
    ticks: u32,
    fates: [usize; 3],
}

fn deck() -> Placing {
    Placing {
        near: 0.2,
        up: 1.0,
        turn: 180.0,
        tilt: 70.0,
        scale: 0.05,
        ..Placing::default()
    }
}

fn slot(place: usize) -> Placing {
    Placing {
        right: SLOT_MIDDLE + (place as f64 - 1.0) * SLOT_STEP,
        up: SLOT_UP,
        near: SLOT_NEAR,
        turn: 180.0,
        scale: CARD_SCALE,
        ..Placing::default()
    }
}

/// Fast at first, and settling.
fn settle(share: f64) -> f64 {
    1.0 - (1.0 - share.clamp(0.0, 1.0)).powi(3)
}

fn floating(placing: Placing, ticks: u32, place: usize) -> Placing {
    let time = f64::from(ticks) * 0.09 + place as f64 * 2.1;
    Placing {
        up: placing.up + time.sin() * 0.012,
        roll: placing.roll + (time * 0.7).cos() * 1.6,
        ..placing
    }
}

fn dealt(place: usize, ticks: u32) -> Placing {
    let begun = ticks.saturating_sub(DEAL_START * place as u32);
    let share = settle(f64::from(begun) / f64::from(DEAL_TICKS));
    let flying = deck().blend(slot(place), share);
    Placing {
        turn: flying.turn - (1.0 - share) * DEAL_LAPS * 360.0,
        up: flying.up + (share * PI).sin() * 0.12,
        ..flying
    }
}

/// The chosen card comes forward, spins, and stops on its face.
fn revealed(place: usize, share: f64) -> Placing {
    let eased = settle(share);
    let end = Placing {
        right: SLOT_MIDDLE,
        up: SLOT_UP + 0.03,
        near: SLOT_NEAR + 0.06,
        turn: 180.0 + (TURN_LAPS + 0.5) * 360.0,
        scale: CARD_SCALE * 1.15,
        ..Placing::default()
    };
    let moving = slot(place).blend(end, eased);
    Placing {
        tilt: (share * PI).sin() * 18.0,
        up: moving.up + (share * PI).sin() * 0.05,
        ..moving
    }
}

/// The others fall away, turning.
fn dropped(place: usize, share: f64) -> Placing {
    let from = slot(place);
    let side = place as f64 - 1.0;
    let side = if side == 0.0 { 1.0 } else { side };
    let end = Placing {
        right: from.right + side * 0.5,
        up: from.up - 0.7,
        turn: from.turn + side * 540.0,
        roll: side * 90.0,
        scale: 0.0,
        ..from
    };
    from.blend(end, share * share)
}

/// Any order of the fates, different from one draw to the next.
fn shuffled() -> [usize; 3] {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.subsec_nanos() as usize / 1000);
    let mut fates = [0, 1, 2];
    fates.swap(0, nanos % 3);
    fates.swap(1, 1 + nanos / 3 % 2);
    fates
}

impl Cards {
    fn place(&self, table: &Table, place: usize, placing: Placing, ticks: u32, cx: &mut Cx) {
        if let Some(prop) = self.props.get(table.fates[place]) {
            cx.show(place as u32, prop, placing, ticks as u16);
        }
    }
}

impl Interface for Cards {
    type State = Table;

    fn install(&mut self, art: &mut ArtBuilder, _theme: &Theme) {
        let (width, height) = CARD_SIZE;
        self.props = (0..FATES.len())
            .map(|symbol| art.card(face(symbol), back(), width, height))
            .collect();
    }

    fn shot(&self) -> Shot {
        Shot::Face
    }

    fn view(&self, table: &Table, _cx: &mut Cx) -> Screen {
        let mut canvas = Canvas::new();
        let mut rows = Vec::new();
        let mut top = TURN_TOP;
        match table.moment {
            Moment::Dealing | Moment::Turning(_) => {}
            Moment::Waiting => {
                canvas.centered(
                    0,
                    from_top(HEADING_Y),
                    "Choisis une carte",
                    TextColor::White,
                );
                let turn = |place: usize| {
                    let action = format!("turn/{place}");
                    Some(Control::action(Text::new("Tourner"), &action))
                };
                rows.push((0..FATES.len()).map(turn).collect());
            }
            Moment::Shown(place) => {
                let (name, saying) = FATES[table.fates[place]];
                canvas.centered(0, from_top(FATE_Y), name, TextColor::Rgb(0xc48cff));
                for (line, words) in saying.iter().enumerate() {
                    let y = from_top(SAYING_Y) + SAYING_STEP * line as i32;
                    canvas.centered(0, y, words, TextColor::White);
                }
                top = THANKS_TOP;
                rows.push(vec![
                    None,
                    Some(Control::action(Text::new("Merci"), "back")),
                    None,
                ]);
            }
        }
        let whole = COLUMN_WIDTH * 3 + GAP * 2;
        Screen {
            canvas,
            grid: Grid {
                x: -whole / 2,
                top,
                widths: vec![COLUMN_WIDTH; FATES.len()],
                rows,
            },
        }
    }

    fn click(&self, table: &mut Table, action: &str, cx: &mut Cx) -> Reply {
        if action == "back" {
            return Reply::Back;
        }
        let place = action.strip_prefix("turn/").and_then(|at| at.parse().ok());
        if let (Moment::Waiting, Some(place)) = (table.moment, place.filter(|at: &usize| *at < 3)) {
            table.moment = Moment::Turning(place);
            table.ticks = 0;
            cx.memory.set("fortune_told");
            cx.play(Sound::new(TURN_SOUND));
            cx.gesture(Gesture::Wave);
        }
        Reply::Stay
    }

    fn tick(&self, table: &mut Table, cx: &mut Cx) -> bool {
        let ticks = table.ticks;
        table.ticks += 1;
        match table.moment {
            Moment::Dealing => {
                if ticks == 0 {
                    table.fates = shuffled();
                    cx.gesture(Gesture::Present);
                }
                if ticks.is_multiple_of(DEAL_START) && ticks < DEAL_START * 3 {
                    cx.play(Sound::new(DEAL_SOUND));
                }
                if ticks.is_multiple_of(STEP) {
                    for place in 0..FATES.len() {
                        self.place(table, place, dealt(place, ticks + STEP), STEP, cx);
                    }
                }
                if ticks >= DEALT_AT {
                    table.moment = Moment::Waiting;
                    return true;
                }
            }
            Moment::Waiting => {
                if ticks.is_multiple_of(CALM_STEP) {
                    for place in 0..FATES.len() {
                        let placing = floating(slot(place), ticks + CALM_STEP, place);
                        self.place(table, place, placing, CALM_STEP, cx);
                    }
                }
            }
            Moment::Turning(chosen) => {
                if ticks.is_multiple_of(SPARK_STEP) {
                    let spot = revealed(chosen, f64::from(ticks) / f64::from(TURN_TICKS));
                    cx.burst(Particle::Enchant, spot, 6);
                }
                if ticks.is_multiple_of(STEP) {
                    let share = f64::from(ticks + STEP) / f64::from(TURN_TICKS);
                    for place in 0..FATES.len() {
                        let placing = if place == chosen {
                            revealed(place, share)
                        } else {
                            dropped(place, share)
                        };
                        self.place(table, place, placing, STEP, cx);
                    }
                }
                if ticks >= TURN_TICKS {
                    (0..FATES.len())
                        .filter(|place| *place != chosen)
                        .for_each(|place| cx.hide(place as u32));
                    table.moment = Moment::Shown(chosen);
                    table.ticks = 0;
                    cx.play(Sound::new(REVEAL_SOUND));
                    cx.burst(Particle::EndRod, revealed(chosen, 1.0), 26);
                    cx.gesture(Gesture::Nod);
                    return true;
                }
            }
            Moment::Shown(chosen) => {
                if ticks.is_multiple_of(CALM_STEP) {
                    let time = f64::from(ticks + CALM_STEP) * 0.07;
                    let still = revealed(chosen, 1.0);
                    let placing = Placing {
                        turn: still.turn + time.sin() * 14.0,
                        ..floating(still, ticks + CALM_STEP, chosen)
                    };
                    self.place(table, chosen, placing, CALM_STEP, cx);
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shuffle_keeps_every_fate() {
        let mut fates = shuffled();
        fates.sort_unstable();
        assert_eq!(fates, [0, 1, 2]);
    }

    #[test]
    fn a_dealt_card_lands_on_its_back_in_its_slot() {
        let landed = dealt(2, DEALT_AT);
        assert_eq!(landed.right, slot(2).right);
        assert_eq!(landed.turn, 180.0);
    }

    #[test]
    fn the_chosen_card_stops_on_its_face() {
        assert_eq!(revealed(0, 1.0).turn.rem_euclid(360.0), 0.0);
    }

    #[test]
    fn no_keyframe_turns_a_card_the_wrong_way() {
        let keyframes = TURN_TICKS / STEP;
        let turns: Vec<f64> = (0..=keyframes)
            .map(|step| revealed(0, f64::from(step) / f64::from(keyframes)).turn)
            .collect();
        assert!(turns.windows(2).all(|pair| pair[1] - pair[0] < 180.0));
        let turns: Vec<f64> = (0..DEALT_AT / STEP)
            .map(|step| dealt(0, step * STEP).turn)
            .collect();
        assert!(
            turns
                .windows(2)
                .all(|pair| (pair[1] - pair[0]).abs() < 180.0)
        );
    }
}
