//! The way between the world and the map. The camera rises out of the world, crosses a
//! short veil of cloud and comes out over the player's marker on the model, then draws back
//! to the view. The way out is the same one, backwards. All of it is client-side gliding:
//! the server gives a camera the end of a segment and the client does the rest.
//!
//! How the view goes there is a [`Flow`], which the intro and the outro choose apart.

use bevy_ecs::prelude::Component;
use voidmc::components::{Position, Rotation};
use voidmc_protocol::clientbound::{ClientboundPacket, SetTitleText, SetTitlesAnimation};

use crate::art;
use crate::pointer::{ease, look_direction};

/// How long the veil takes to come and to go, in ticks.
pub const VEIL_FADE: u32 = 5;
/// Longer than anything has to hide. The veil is lifted by hand long before.
const VEIL_STAY: u32 = 400;
const RISE_TICKS: u32 = 20;
const LANDING_TICKS: u32 = 18;
/// From the player's marker, up to the view.
pub const OPENING_TICKS: u32 = 30;
/// The other way, on the way out.
pub const CLOSING_TICKS: u32 = 24;
/// How long a camera takes over a segment of a flight. Display entities glide for up to
/// 59 ticks. A segment is a straight line, which a smooth flight wants short enough.
pub const SEGMENT_TICKS: u32 = 5;
/// A camera is told to take this much longer than a segment lasts, so that the next one
/// comes before it has stopped: it never stalls between two, and its corners round off.
const GLIDE_OVERLAP: u32 = 2;
/// A new camera shows up on the client by this tick, with its look rounded to 1.4 degrees,
/// and gets the exact one.
pub const TURN_TICKS: u32 = 2;
/// A client takes 3 ticks to turn an entity. Then nothing shows when the view goes over.
pub const SWAP_TICKS: u32 = TURN_TICKS + 4;
/// The engine's last move goes out up to a tick late, and would land on top of the exact
/// place sent before it: that goes out this long after the end of the last segment.
const SETTLE_TICKS: u32 = 2;
/// What a camera on a flight is set to.
pub const FLIGHT_GLIDE_TICKS: u8 = (SEGMENT_TICKS + GLIDE_OVERLAP) as u8;
/// How far over the player the camera goes, in blocks.
const FLIGHT_HEIGHT: f64 = 34.0;
/// Straight down.
const DOWN: f32 = 90.0;
/// Straight up.
const UP: f32 = -90.0;
/// How wide a spiral gets halfway, on the world and at the height of a view.
const SPIRAL_RADIUS: f64 = 7.0;
const SPIRAL_MODEL_SHARE: f64 = 0.2;
/// How far ahead a glide goes in the world, and how far over a straight line.
const GLIDE_REACH: f64 = 52.0;
const GLIDE_BULGE: f64 = 7.0;
/// How low a glide looks, down from the horizon, where it meets the land.
const GLIDE_PITCH: f32 = 34.0;
const GLIDE_LOOK: f32 = 58.0;
/// How far a glide ends from the marker, in stops of a dive.
const GLIDE_STOPS: f64 = 3.0;
/// A leap goes higher than the others, into the sky.
const LEAP_HEIGHT: f64 = 56.0;
/// A leap falls on the view from this share of its height over it.
const LEAP_MODEL_SHARE: f64 = 0.9;

/// What brings the veil over the screen. A title is the one thing that covers it
/// from the server and fades on its own.
pub fn veil_in() -> [ClientboundPacket; 2] {
    [
        SetTitlesAnimation {
            fade_in: VEIL_FADE as i32,
            stay: VEIL_STAY as i32,
            fade_out: VEIL_FADE as i32,
        }
        .into(),
        SetTitleText {
            text: voidmc_ui::Text::new(art::veil()).to_nbt(),
        }
        .into(),
    ]
}

/// New times restart the title's clock, here on its last stretch.
pub fn veil_out() -> ClientboundPacket {
    SetTitlesAnimation {
        fade_in: 0,
        stay: 0,
        fade_out: VEIL_FADE as i32,
    }
    .into()
}

/// One way for the view to go between the world and the map. The intro and the outro each
/// have one, and an outro is the flight of its intro, backwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Flow {
    /// Straight up, tilting down to the ground under the player, and over their marker.
    #[default]
    Climb,
    /// Up a widening spiral, turning a whole circle, and down one on the model.
    Spiral,
    /// Ahead and up like a bird, then in over the model from low on the horizon.
    Glide,
    /// Eyes on the sky and up into it, then a fall onto the view from high above.
    Leap,
}

impl Flow {
    pub const ALL: [Self; 4] = [Self::Climb, Self::Spiral, Self::Glide, Self::Leap];

    pub fn label(self) -> &'static str {
        match self {
            Self::Climb => "Envol",
            Self::Spiral => "Spirale",
            Self::Glide => "Vol plané",
            Self::Leap => "Saut dans le ciel",
        }
    }

    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|flow| *flow == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    fn extra_ticks(self) -> u32 {
        match self {
            Self::Climb => 0,
            Self::Spiral => 14,
            Self::Glide => 12,
            Self::Leap => 6,
        }
    }

    /// Out of an eye, into the sky of the world.
    pub fn rise(self, eye: Position, look: Rotation) -> Flight {
        let start = [eye.x, eye.y, eye.z];
        let [yaw, pitch] = [look.yaw, look.pitch];
        let up = |height| [eye.x, eye.y + height, eye.z];
        let length = RISE_TICKS + self.extra_ticks();
        match self {
            Self::Climb => {
                Flight::new(start, up(FLIGHT_HEIGHT), [yaw, pitch], length).turning_to([yaw, DOWN])
            }
            Self::Spiral => Flight::new(start, up(FLIGHT_HEIGHT), [yaw, pitch], length)
                .turning_to([yaw + 360.0, GLIDE_LOOK])
                .along(Shape::Spiral {
                    radius: SPIRAL_RADIUS,
                    turns: 1.0,
                    phase: yaw,
                }),
            Self::Glide => {
                let way = look_direction(yaw, 0.0);
                let to = [
                    eye.x + way[0] * GLIDE_REACH,
                    eye.y + FLIGHT_HEIGHT,
                    eye.z + way[2] * GLIDE_REACH,
                ];
                Flight::new(start, to, [yaw, pitch], length)
                    .turning_to([yaw, GLIDE_LOOK])
                    .along(Shape::Bulge([0.0, GLIDE_BULGE, 0.0]))
            }
            Self::Leap => {
                Flight::new(start, up(LEAP_HEIGHT), [yaw, pitch], length).turning_to([yaw, UP])
            }
        }
    }

    /// Back down to an eye, by the same way.
    pub fn landing(self, eye: Position, look: Rotation) -> Flight {
        Flight {
            length: LANDING_TICKS + self.extra_ticks(),
            ..self.rise(eye, look).reversed()
        }
    }

    /// From the view to the end of the flight over the model, `stop` blocks off a marker.
    /// An intro takes this one backwards.
    pub fn to_marker(
        self,
        eye: [f64; 3],
        look: [f32; 2],
        marker: [f64; 3],
        stop: f64,
        length: u32,
    ) -> Flight {
        let length = length + self.extra_ticks();
        let [yaw, _] = look;
        let over = [marker[0], marker[1] + stop, marker[2]];
        let high = eye[1] - marker[1];
        match self {
            Self::Climb => Flight::new(eye, over, look, length).turning_to([yaw, DOWN]),
            Self::Spiral => Flight::new(eye, over, look, length)
                .turning_to([yaw - 360.0, DOWN])
                .along(Shape::Spiral {
                    radius: high * SPIRAL_MODEL_SHARE,
                    turns: -1.0,
                    phase: yaw,
                }),
            Self::Glide => {
                let way = look_direction(yaw, GLIDE_PITCH);
                let back = stop * GLIDE_STOPS;
                let to = std::array::from_fn(|axis| marker[axis] - way[axis] * back);
                Flight::new(eye, to, look, length).turning_to([yaw, GLIDE_PITCH])
            }
            Self::Leap => {
                let to = [eye[0], eye[1] + high * LEAP_MODEL_SHARE, eye[2]];
                Flight::new(eye, to, look, length).turning_to([yaw, DOWN])
            }
        }
    }
}

/// What a flight does on top of a straight line. It starts and ends on that line, so that
/// nothing changes where a flight begins and arrives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    Straight,
    /// Off to this side, most of all halfway.
    Bulge([f64; 3]),
    /// Around the line, this far halfway, starting towards `phase` as a yaw.
    Spiral {
        radius: f64,
        turns: f64,
        phase: f32,
    },
}

impl Shape {
    /// How far off the line, at this share of the way.
    fn offset(self, share: f64) -> [f64; 3] {
        let swell = (share * std::f64::consts::PI).sin();
        match self {
            Self::Straight => [0.0; 3],
            Self::Bulge(side) => side.map(|axis| axis * swell),
            Self::Spiral {
                radius,
                turns,
                phase,
            } => {
                let angle = f64::from(phase).to_radians() + share * turns * std::f64::consts::TAU;
                [
                    -angle.sin() * radius * swell,
                    0.0,
                    angle.cos() * radius * swell,
                ]
            }
        }
    }
}

/// A camera on its way somewhere. It gets told where to be a segment ahead and the client
/// glides it there by itself, so that a whole flight takes a handful of packets. Its look
/// goes the same way, except for an eye: what rides a camera turns by a packet a tick.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Flight {
    from: [f64; 3],
    to: [f64; 3],
    look: [f32; 2],
    look_to: [f32; 2],
    shape: Shape,
    /// It runs from `to` to `from`, for a shape to go back on itself.
    backwards: bool,
    length: u32,
    ticks: u32,
}

impl Flight {
    pub fn new(from: [f64; 3], to: [f64; 3], look: [f32; 2], length: u32) -> Self {
        Self {
            from,
            to,
            look,
            look_to: look,
            shape: Shape::Straight,
            backwards: false,
            length,
            ticks: 0,
        }
    }

    /// Ends on this look.
    pub fn turning_to(self, look_to: [f32; 2]) -> Self {
        Self { look_to, ..self }
    }

    pub fn along(self, shape: Shape) -> Self {
        Self { shape, ..self }
    }

    /// From an eye to `stop` blocks short of a target, which ends up in the middle of the
    /// screen.
    pub fn dive(eye: [f64; 3], look: [f32; 2], target: [f64; 3], stop: f64, length: u32) -> Self {
        let way = look_direction(look[0], look[1]);
        let to = std::array::from_fn(|axis| target[axis] - way[axis] * stop);
        Self::new(eye, to, look, length)
    }

    /// The same way, the other way round.
    pub fn reversed(self) -> Self {
        Self {
            backwards: !self.backwards,
            ..self
        }
    }

    pub fn tick(&mut self) {
        self.ticks = (self.ticks + 1).min(self.last_ticks());
    }

    /// When the camera gets told about the end of the flight.
    fn last_segment(&self) -> u32 {
        (self.length - 1) / SEGMENT_TICKS * SEGMENT_TICKS
    }

    /// The tick it is sure to be there, with the last correction glided out and one to spare.
    fn last_ticks(&self) -> u32 {
        self.last_segment() + SETTLE_TICKS + 1 + u32::from(FLIGHT_GLIDE_TICKS)
    }

    /// The ticks before the camera is there, gliding included. Past that the eye can take
    /// over from it, and sees exactly the same.
    pub fn left(&self) -> u32 {
        self.last_ticks() - self.ticks
    }

    /// The ticks of the flight itself still to go.
    pub fn flying_for(&self) -> u32 {
        self.length.saturating_sub(self.ticks)
    }

    /// The moment to give the camera the exact end of the flight, after the engine's move.
    pub fn settling(&self) -> bool {
        self.ticks == self.last_segment() + SETTLE_TICKS
    }

    /// Where the flight ends.
    pub fn end(&self) -> [f64; 3] {
        self.pose(self.length)
    }

    /// How long the flight is, without the time it takes to come to rest.
    pub fn length(&self) -> u32 {
        self.length
    }

    fn pose(&self, ticks: u32) -> [f64; 3] {
        let progress = ease(f64::from(ticks.min(self.length)) / f64::from(self.length));
        let share = if self.backwards {
            1.0 - progress
        } else {
            progress
        };
        let off = self.shape.offset(share);
        std::array::from_fn(|axis| {
            self.from[axis] + (self.to[axis] - self.from[axis]) * share + off[axis]
        })
    }

    /// The look at some tick. It waits for the view to come over, as the place does, and
    /// then has all of the flight to turn.
    fn look_at(&self, ticks: u32) -> [f32; 2] {
        let turning = ticks.saturating_sub(SWAP_TICKS).min(self.length);
        let length = self.length.saturating_sub(SWAP_TICKS).max(1);
        let progress = ease(f64::from(turning.min(length)) / f64::from(length)) as f32;
        let share = if self.backwards {
            1.0 - progress
        } else {
            progress
        };
        std::array::from_fn(|axis| self.look[axis] + (self.look_to[axis] - self.look[axis]) * share)
    }

    /// Where the view is now.
    pub fn at(&self) -> [f64; 3] {
        self.pose(self.ticks)
    }

    /// How the view looks, as yaw and pitch.
    pub fn looking(&self) -> [f32; 2] {
        self.look_at(self.ticks)
    }

    /// How the camera entity is told to look: at the end of the segment under way, with
    /// its place, so that the client glides both at once.
    pub fn target_look(&self) -> [f32; 2] {
        if self.ticks < SWAP_TICKS {
            return self.look_at(0);
        }
        self.look_at((self.ticks / SEGMENT_TICKS + 1) * SEGMENT_TICKS)
    }

    /// The look it starts on.
    pub fn first_look(&self) -> [f32; 2] {
        self.look_at(0)
    }

    /// The place it starts from.
    pub fn start(&self) -> [f64; 3] {
        self.pose(0)
    }

    /// An eye has its look told tick by tick, from the moment it is seen through until it
    /// has got to the end.
    pub fn eye_turning(&self) -> bool {
        self.ticks >= SWAP_TICKS && self.ticks <= self.length + 1 && self.look != self.look_to
    }

    /// Where the camera entity is told to be: the end of the segment under way. It changes
    /// once a segment and not before, which is what keeps the packets down.
    pub fn target(&self) -> [f64; 3] {
        if self.ticks < SWAP_TICKS {
            return self.pose(0);
        }
        self.pose((self.ticks / SEGMENT_TICKS + 1) * SEGMENT_TICKS)
    }

    /// The moment to give what sees for an eye its exact look.
    pub fn turning(&self) -> bool {
        self.ticks == TURN_TICKS
    }

    /// The moment the view goes over to the camera, which has got that look by then.
    pub fn swapping(&self) -> bool {
        self.ticks == SWAP_TICKS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EYE: Position = Position {
        x: 10.0,
        y: 65.62,
        z: -4.0,
    };
    const LOOK: Rotation = Rotation {
        yaw: 170.0,
        pitch: 12.0,
    };

    #[test]
    fn a_rise_starts_exactly_where_the_player_sees() {
        for flow in Flow::ALL {
            let flight = flow.rise(EYE, LOOK);
            assert_eq!(flight.at(), [EYE.x, EYE.y, EYE.z]);
            assert_eq!(flight.looking(), [LOOK.yaw, LOOK.pitch]);
        }
    }

    #[test]
    fn a_landing_ends_exactly_where_the_player_sees() {
        for flow in Flow::ALL {
            let mut flight = flow.landing(EYE, LOOK);
            while flight.left() > 0 {
                flight.tick();
            }
            for axis in 0..3 {
                assert!((flight.at()[axis] - [EYE.x, EYE.y, EYE.z][axis]).abs() < 1e-9);
            }
            assert_eq!(flight.target(), flight.end());
            assert_eq!(flight.looking(), [LOOK.yaw, LOOK.pitch]);
        }
    }

    /// What Léo asked for: the ground under the player, by the end of the way up.
    #[test]
    fn the_first_flow_ends_looking_straight_down() {
        let mut flight = Flow::Climb.rise(EYE, LOOK);
        let mut pitches = vec![flight.looking()[1]];
        while flight.left() > 0 {
            flight.tick();
            pitches.push(flight.looking()[1]);
        }
        assert_eq!(flight.looking(), [LOOK.yaw, DOWN]);
        assert!(pitches.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    /// A turn of a tick has to stay something the eye can follow.
    #[test]
    fn no_flow_turns_by_more_than_a_client_can_smooth() {
        for flow in Flow::ALL {
            let mut flight = flow.rise(EYE, LOOK);
            let mut last = flight.looking();
            while flight.left() > 0 {
                flight.tick();
                let now = flight.looking();
                let turn = (now[0] - last[0]).abs().max((now[1] - last[1]).abs());
                assert!(turn < 40.0, "{flow:?} turns {turn} in a tick");
                last = now;
            }
        }
    }

    /// An intro and its outro are the same flight, and each is really its own way.
    #[test]
    fn each_flow_flies_its_own_way() {
        let middles: Vec<_> = Flow::ALL
            .iter()
            .map(|flow| {
                let mut flight = flow.rise(EYE, LOOK);
                for _ in 0..flight.length() / 2 {
                    flight.tick();
                }
                (flight.at(), flight.looking())
            })
            .collect();
        for (index, one) in middles.iter().enumerate() {
            assert!(!middles[index + 1..].contains(one));
        }
    }

    #[test]
    fn a_flow_on_the_model_ends_on_the_view_when_taken_backwards() {
        let eye = [10.0, 120.0, 50.0];
        let look = [180.0, 78.75];
        for flow in Flow::ALL {
            let mut flight = flow
                .to_marker(eye, look, [0.0, 70.0, 0.0], 16.0, 24)
                .reversed();
            while flight.left() > 0 {
                flight.tick();
            }
            for axis in 0..3 {
                assert!((flight.at()[axis] - eye[axis]).abs() < 1e-9);
            }
            assert_eq!(flight.looking(), look);
        }
    }

    #[test]
    fn the_next_flow_goes_round_them_all() {
        let mut flow = Flow::default();
        for expected in Flow::ALL.iter().cycle().skip(1).take(4) {
            flow = flow.next();
            assert_eq!(flow, *expected);
        }
    }

    /// A camera that had not stopped would jump when the eye takes over from it.
    #[test]
    fn a_camera_has_stopped_before_the_flight_ends() {
        let mut flight = Flow::Climb.rise(EYE, LOOK);
        while !flight.settling() {
            flight.tick();
        }
        assert!(flight.left() > u32::from(FLIGHT_GLIDE_TICKS));
    }

    #[test]
    fn a_flight_waits_for_the_view_to_come_over() {
        let mut flight = Flow::Climb.rise(EYE, LOOK);
        for _ in 0..SWAP_TICKS - 1 {
            flight.tick();
            assert_eq!(flight.target(), [EYE.x, EYE.y, EYE.z]);
        }
    }

    /// One packet per segment is the whole point of this.
    #[test]
    fn a_flight_changes_its_target_once_a_segment() {
        let mut flight = Flow::Climb.rise(EYE, LOOK);
        let mut targets = vec![flight.target()];
        for _ in 0..RISE_TICKS {
            flight.tick();
            if targets.last() != Some(&flight.target()) {
                targets.push(flight.target());
            }
        }
        assert_eq!(
            targets.len() as u32,
            (RISE_TICKS - SWAP_TICKS).div_ceil(SEGMENT_TICKS) + 1
        );
        assert_eq!(*targets.last().unwrap(), flight.end());
    }

    /// A client jumps past this distance instead of gliding, and a dive is the longest
    /// flight there is: from as far as a view goes, down to a waypoint.
    #[test]
    fn no_segment_of_a_long_dive_makes_a_client_jump() {
        let mut flight = Flight::dive(
            [0.0, 190.0, 0.0],
            [180.0, 50.0],
            [90.0, 70.0, 90.0],
            16.0,
            crate::pointer::DIVE_TICKS,
        );
        let mut last = flight.target();
        for _ in 0..crate::pointer::DIVE_TICKS {
            flight.tick();
            let next = flight.target();
            let moved = (0..3)
                .map(|axis| (next[axis] - last[axis]).powi(2))
                .sum::<f64>();
            assert!(
                moved.sqrt() < crate::pointer::CLIENT_SNAP_DISTANCE,
                "{moved}"
            );
            last = next;
        }
    }

    /// Slow, then fast, then slow: both ends are soft, and it never goes back.
    #[test]
    fn a_dive_speeds_up_and_slows_down() {
        let length = crate::pointer::DIVE_TICKS;
        let mut flight = Flight::dive(
            [0.0, 100.0, 40.0],
            [150.0, 40.0],
            [10.0, 70.0, 0.0],
            16.0,
            length,
        );
        let mut steps = Vec::new();
        let mut last = flight.at()[1];
        for _ in 0..length {
            flight.tick();
            steps.push(last - flight.at()[1]);
            last = flight.at()[1];
        }
        let middle = steps[steps.len() / 2];
        assert!(steps.iter().all(|step| *step >= 0.0));
        assert!(steps[0] < middle / 10.0 && steps[steps.len() - 1] < middle / 10.0);
    }

    #[test]
    fn a_dive_ends_with_its_target_in_the_middle_of_the_screen() {
        let target = [10.0, 70.0, 0.0];
        let mut flight = Flight::dive([0.0, 100.0, 40.0], [150.0, 40.0], target, 16.0, 12);
        while flight.left() > 0 {
            flight.tick();
        }
        let way = look_direction(150.0, 40.0);
        let at = flight.at();
        for axis in 0..3 {
            assert!((at[axis] + way[axis] * 16.0 - target[axis]).abs() < 1e-9);
        }
    }

    #[test]
    fn the_veil_is_whole_before_the_camera_stops() {
        const { assert!(VEIL_FADE < RISE_TICKS && VEIL_FADE < LANDING_TICKS) };
    }
}
