//! Going from one point of interest to the next, instead of sliding over the map.
//!
//! [`Navigation::Jumps`] takes the view away from the mouse: a flick goes to the nearest
//! [`Point`] that way. [`list`] offers them all, in a dialog, however the view moves.
//! Whatever picked it, a [`Target`] stays in the middle of the screen as long as it is the
//! target, so that one who walks is followed. In [`Navigation::Free`] the mouse lets go of
//! it, and the navigation never changed. [`Goto`] holds that, and `plugin::goto` feeds it
//! from the session.
//!
//! Nothing here knows of the height of the view, so a zoom goes on at any time.
//!
//! The client decides what a dialog looks like and always centres it, and an empty cell
//! next to the list could only be a button, with a sprite and a stop for the Tab key. So
//! the list stays in the middle, and it is the view that makes way: [`LIST_SHIFT`].

use voidmc::TextColor;
use voidmc_ui::{Button, Dialog, Text};

use crate::pointer::MAX_VIEW_STEP;

const NAMESPACE: &str = "voidmap:goto";
/// The boss bar sits right where a title goes.
const TITLE: &str = " ";
const PLAYERS: &str = "Joueurs";
const PLACES: &str = "Lieux";
/// What the player's own point is called.
pub const YOU: &str = "Moi";
const CLOSE: &str = "Fermer";
const ROW_WIDTH: i32 = 130;

/// How far the view moves west of its target while the list is open, for a height of 1.
/// It puts the target right of the list: about three quarters across a 5:4 window, and
/// a wider one shows it closer to the list, which then takes less of the width.
pub const LIST_SHIFT: f64 = 0.42;

/// Degrees the mouse goes in one gesture for that to be a flick. Under this it is a hand
/// resting on the mouse.
const FLICK_DEGREES: f64 = 3.0;
/// Ticks with no report that end a gesture. One gesture is one jump, however long it is.
const GESTURE_OVER_TICKS: u32 = 5;
/// How far from the way of a flick a point can be, as the cosine of the angle: 60°.
const WIDEST_FLICK: f64 = 0.5;
/// A point closer than this is where the view already is.
const HERE: f64 = 3.0;
/// About how long the view takes to get there, once it is not limited by its speed.
const GLIDE_TICKS: f64 = 8.0;
/// Under this the view has arrived, and stops being sent.
const ARRIVED: f64 = 0.02;

/// How the mouse moves the view of a pan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Navigation {
    /// The view slides under the mouse.
    #[default]
    Free,
    /// The view goes from point to point.
    Jumps,
}

impl Navigation {
    pub const ALL: [Self; 2] = [Self::Free, Self::Jumps];

    pub fn label(self) -> &'static str {
        match self {
            Self::Free => "Libre",
            Self::Jumps => "Par sauts",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Free => Self::Jumps,
            Self::Jumps => Self::Free,
        }
    }
}

/// What a view can go to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The player's own marker.
    You,
    /// By the id of [`crate::players::Characters`].
    Character(u128),
    /// By its block of the world: the models do not all have the same waypoints.
    Waypoint([i32; 2]),
}

/// A target as it stands now.
#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    pub target: Target,
    pub name: String,
    /// Where the view has to be for it to be in the middle of the screen.
    pub at: [f64; 2],
    /// A place the player cannot go to yet.
    pub closed: bool,
}

/// Where the view is going, and how the mouse and keys get it there.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Goto {
    target: Option<Target>,
    /// What the player knows it as.
    name: Option<String>,
    /// In blocks per tick.
    speed: [f64; 2],
    /// How far the mouse went in this gesture, in degrees.
    gesture: [f64; 2],
    /// Ticks since the mouse last moved.
    idle: u32,
    /// This gesture has made its jump.
    spent: bool,
    /// Forward, back and jump as last reported, to act on a press and not on a repeat.
    pub(crate) keys: [bool; 3],
    /// The list is on screen.
    pub(crate) listing: bool,
    /// What the hover goes by while the view is held off the target, on the model.
    pub(crate) held_on: Option<[f64; 3]>,
    /// What the client's tab list was last told.
    pub(crate) told: Option<Option<String>>,
}

impl Goto {
    pub fn target(&self) -> Option<Target> {
        self.target
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Sets off for a point. A view on its way keeps its speed: it curves, and never stops.
    pub fn choose(&mut self, point: &Point) {
        self.target = Some(point.target);
        self.name = Some(point.name.clone());
    }

    pub fn clear(&mut self) {
        self.target = None;
        self.name = None;
        self.speed = [0.0; 2];
        self.held_on = None;
    }

    /// Leaves the target and the gesture, for a player who changes how the view moves.
    pub fn start_over(&mut self) {
        *self = Self {
            told: self.told.take(),
            ..Self::default()
        };
    }

    /// Takes in what the mouse moved.
    pub fn push(&mut self, delta: [f32; 2]) {
        self.gesture[0] += f64::from(delta[0]);
        self.gesture[1] += f64::from(delta[1]);
        self.idle = 0;
    }

    /// The way of a flick that has just got far enough, as a unit vector of the map:
    /// the view looks north, so right is east and down is south.
    pub fn flick(&mut self) -> Option<[f64; 2]> {
        self.idle += 1;
        if self.idle > GESTURE_OVER_TICKS {
            self.gesture = [0.0; 2];
            self.spent = false;
        }
        let length = self.gesture[0].hypot(self.gesture[1]);
        if self.spent || length < FLICK_DEGREES {
            return None;
        }
        self.spent = true;
        Some(self.gesture.map(|axis| axis / length))
    }

    /// Where the view goes on this tick: `SmoothDamp`, which starts and ends on a curve,
    /// keeps its speed through a change of goal, and follows a walker with no visible lag.
    pub fn approach(&mut self, view: [f64; 2], goal: [f64; 2]) -> [f64; 2] {
        let omega = 2.0 / GLIDE_TICKS;
        let decay = 1.0 / (1.0 + omega + 0.48 * omega.powi(2) + 0.235 * omega.powi(3));
        let off = [view[0] - goal[0], view[1] - goal[1]];
        let kept = (MAX_VIEW_STEP * GLIDE_TICKS / off[0].hypot(off[1]).max(f64::EPSILON)).min(1.0);
        let next: [f64; 2] = std::array::from_fn(|axis| {
            let off = off[axis] * kept;
            let push = self.speed[axis] + omega * off;
            self.speed[axis] = (self.speed[axis] - omega * push) * decay;
            view[axis] - off + (off + push) * decay
        });
        let step = [next[0] - view[0], next[1] - view[1]];
        let length = step[0].hypot(step[1]);
        let slowed = (MAX_VIEW_STEP / length.max(f64::EPSILON)).min(1.0);
        let at = [view[0] + step[0] * slowed, view[1] + step[1] * slowed];
        let left = (at[0] - goal[0]).hypot(at[1] - goal[1]);
        if left < ARRIVED && self.speed[0].hypot(self.speed[1]) < ARRIVED {
            self.speed = [0.0; 2];
            return goal;
        }
        at
    }
}

/// The closest point the way of a flick from `from`. Being off that way counts as being
/// further, so that the point aimed at wins over one that only happens to be near.
pub fn towards(
    points: &[Point],
    from: [f64; 2],
    current: Option<Target>,
    way: [f64; 2],
) -> Option<&Point> {
    let cost = |point: &Point| {
        let to = [point.at[0] - from[0], point.at[1] - from[1]];
        let distance = to[0].hypot(to[1]);
        let along = (to[0] * way[0] + to[1] * way[1]) / distance.max(f64::EPSILON);
        (distance >= HERE && along >= WIDEST_FLICK).then(|| distance / along.powi(2))
    };
    points
        .iter()
        .filter(|point| Some(point.target) != current)
        .filter_map(|point| Some((point, cost(point)?)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(point, _)| point)
}

/// The point after or before the current one in the list, and around. With none, the
/// closest to the view.
pub fn stepped(
    points: &[Point],
    current: Option<Target>,
    view: [f64; 2],
    forward: bool,
) -> Option<&Point> {
    let at = current.and_then(|target| points.iter().position(|p| p.target == target));
    let Some(at) = at else {
        let distance = |p: &Point| (p.at[0] - view[0]).hypot(p.at[1] - view[1]);
        return points
            .iter()
            .min_by(|a, b| distance(a).total_cmp(&distance(b)));
    };
    let step = if forward { 1 } else { points.len() - 1 };
    points.get((at + step) % points.len())
}

/// What a player can ask for by clicking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Go(Target),
    Close,
}

impl Action {
    pub(crate) fn id(self) -> String {
        match self {
            Self::Go(Target::You) => format!("{NAMESPACE}/you"),
            Self::Go(Target::Character(id)) => format!("{NAMESPACE}/character/{id}"),
            Self::Go(Target::Waypoint([x, z])) => format!("{NAMESPACE}/waypoint/{x}/{z}"),
            Self::Close => format!("{NAMESPACE}/close"),
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        let path = id.strip_prefix(NAMESPACE)?.strip_prefix('/')?;
        let mut parts = path.split('/');
        let action = match parts.next()? {
            "you" => Self::Go(Target::You),
            "character" => Self::Go(Target::Character(parts.next()?.parse().ok()?)),
            "waypoint" => {
                let block = [parts.next()?.parse().ok()?, parts.next()?.parse().ok()?];
                Self::Go(Target::Waypoint(block))
            }
            "close" => Self::Close,
            _ => return None,
        };
        parts.next().is_none().then_some(action)
    }
}

/// A row that says what comes under it. The client has no other way to put a text there.
fn heading(text: &str) -> Button {
    Button::with_label(Text::new(text).color(TextColor::Aqua)).width(ROW_WIDTH)
}

fn row(point: &Point) -> Button {
    let label = Text::new(point.name.as_str());
    let label = if point.closed {
        label.color(TextColor::Gray)
    } else {
        label
    };
    Button::with_label(label)
        .width(ROW_WIDTH)
        .on_click(Action::Go(point.target).id())
}

/// Every point, one under the other. The client scrolls it when it does not fit, with the
/// wheel, or by itself as the Tab key and the arrows go down it.
///
/// It says nothing of the current target, and is not sent again once open: a new dialog
/// would start again from the top of the list and take the Tab key's place away.
pub fn list(points: &[Point]) -> Dialog {
    let is_place = |point: &&Point| matches!(point.target, Target::Waypoint(_));
    let (places, people): (Vec<&Point>, Vec<&Point>) = points.iter().partition(is_place);
    let people = people.into_iter().map(row);
    let places = places.into_iter().map(row);
    let rows = people
        .chain(std::iter::once(heading(PLACES)))
        .chain(places)
        .fold(
            Dialog::new(Text::new(TITLE), heading(PLAYERS)),
            Dialog::button,
        );
    rows.columns(1).exit(
        Button::new(CLOSE)
            .width(ROW_WIDTH)
            .on_click(Action::Close.id()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(target: Target, at: [f64; 2]) -> Point {
        Point {
            target,
            name: format!("{target:?}"),
            at,
            closed: false,
        }
    }

    fn cross() -> Vec<Point> {
        vec![
            point(Target::You, [0.0, 0.0]),
            point(Target::Character(1), [50.0, 5.0]),
            point(Target::Character(2), [-30.0, 0.0]),
            point(Target::Waypoint([1, 2]), [0.0, -40.0]),
            point(Target::Waypoint([3, 4]), [100.0, 0.0]),
        ]
    }

    fn glide(goto: &mut Goto, from: [f64; 2], goal: [f64; 2], ticks: u32) -> Vec<[f64; 2]> {
        let mut view = from;
        (0..ticks)
            .map(|_| {
                view = goto.approach(view, goal);
                view
            })
            .collect()
    }

    fn steps(views: &[[f64; 2]]) -> Vec<f64> {
        views
            .windows(2)
            .map(|pair| (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]))
            .collect()
    }

    #[test]
    fn a_view_arrives_exactly_and_stays() {
        let views = glide(&mut Goto::default(), [0.0, 0.0], [120.0, -60.0], 120);
        assert_eq!(views[100], [120.0, -60.0]);
        assert_eq!(views[119], [120.0, -60.0]);
    }

    #[test]
    fn a_glide_stays_under_what_a_client_can_follow() {
        let views = glide(&mut Goto::default(), [-150.0, 0.0], [150.0, 0.0], 120);
        let most = steps(&views).into_iter().fold(0.0, f64::max);
        assert!(most <= MAX_VIEW_STEP + 1e-9, "{most}");
    }

    #[test]
    fn a_glide_crosses_the_map_within_three_seconds() {
        let views = glide(&mut Goto::default(), [-150.0, 0.0], [150.0, 0.0], 60);
        assert!((views[59][0] - 150.0).abs() < 1.0, "{:?}", views[59]);
    }

    /// No jolt at either end: a step is never much larger than the one before when it
    /// starts, and it only ever gets smaller on the way in.
    #[test]
    fn a_glide_starts_and_ends_on_a_curve() {
        let views = glide(&mut Goto::default(), [0.0, 0.0], [40.0, 0.0], 80);
        let steps = steps(&views);
        let top = steps.iter().copied().fold(0.0, f64::max);
        assert!(views[0][0] < top, "starts at full speed");
        let peak = steps.iter().position(|step| *step == top).unwrap();
        assert!(
            steps[peak..]
                .windows(2)
                .all(|pair| pair[1] <= pair[0] + ARRIVED)
        );
    }

    #[test]
    fn a_new_goal_does_not_stop_the_view() {
        let mut goto = Goto::default();
        let views = glide(&mut goto, [0.0, 0.0], [100.0, 0.0], 8);
        let before = steps(&views).pop().unwrap();
        let view = views[7];
        let next = goto.approach(view, [100.0, 80.0]);
        assert!(next[0] - view[0] > before / 2.0);
    }

    #[test]
    fn a_walker_is_followed_closely() {
        let mut goto = Goto::default();
        let mut view = [0.0, 0.0];
        for tick in 0..200 {
            let goal = [f64::from(tick) * 0.1, 0.0];
            view = goto.approach(view, goal);
        }
        assert!((19.9 - view[0]).abs() < 1.0, "{view:?}");
    }

    #[test]
    fn one_gesture_is_one_jump() {
        let mut goto = Goto::default();
        goto.push([2.0, 0.0]);
        assert_eq!(goto.flick(), None);
        goto.push([2.0, 0.0]);
        assert_eq!(goto.flick(), Some([1.0, 0.0]));
        goto.push([30.0, 0.0]);
        assert_eq!(goto.flick(), None);
        for _ in 0..=GESTURE_OVER_TICKS {
            assert_eq!(goto.flick(), None);
        }
        goto.push([0.0, -4.0]);
        assert_eq!(goto.flick(), Some([0.0, -1.0]));
    }

    #[test]
    fn a_shaking_hand_never_jumps() {
        let mut goto = Goto::default();
        for _ in 0..50 {
            goto.push([0.4, 0.0]);
            assert_eq!(goto.flick(), None);
            goto.push([-0.4, 0.0]);
            assert_eq!(goto.flick(), None);
        }
    }

    #[test]
    fn a_flick_goes_to_the_nearest_that_way() {
        let points = cross();
        let east = towards(&points, [0.0, 0.0], Some(Target::You), [1.0, 0.0]);
        assert_eq!(east.map(|p| p.target), Some(Target::Character(1)));
        let north = towards(&points, [0.0, 0.0], Some(Target::You), [0.0, -1.0]);
        assert_eq!(north.map(|p| p.target), Some(Target::Waypoint([1, 2])));
    }

    #[test]
    fn a_flick_at_nothing_stays() {
        let points = cross();
        assert_eq!(towards(&points, [0.0, 0.0], None, [0.0, 1.0]), None);
    }

    #[test]
    fn keys_go_round_the_list() {
        let points = cross();
        let after = stepped(&points, Some(Target::You), [0.0, 0.0], true);
        assert_eq!(after.map(|p| p.target), Some(Target::Character(1)));
        let before = stepped(&points, Some(Target::You), [0.0, 0.0], false);
        assert_eq!(before.map(|p| p.target), Some(Target::Waypoint([3, 4])));
    }

    #[test]
    fn with_no_target_a_key_starts_from_the_closest() {
        let points = cross();
        let first = stepped(&points, None, [90.0, 0.0], true);
        assert_eq!(first.map(|p| p.target), Some(Target::Waypoint([3, 4])));
    }

    #[test]
    fn an_action_survives_its_id() {
        for action in [
            Action::Go(Target::You),
            Action::Go(Target::Character(u128::MAX)),
            Action::Go(Target::Waypoint([-12, 40])),
            Action::Close,
        ] {
            assert_eq!(Action::parse(&action.id()), Some(action));
        }
    }

    #[test]
    fn other_ids_are_left_to_others() {
        for id in [
            "voidmap:settings/close",
            "voidmap:goto/waypoint/1",
            "voidmap:goto/close/1",
        ] {
            assert_eq!(Action::parse(id), None);
        }
    }
}
