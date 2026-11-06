//! The way between the world and the map. The camera rises out of the world, crosses a
//! short veil of cloud and comes out over the player's marker on the model, then draws back
//! to the view. The way out is the same one, backwards. All of it is client-side gliding:
//! the server gives a camera the end of a segment and the client does the rest.

use bevy_ecs::prelude::Component;
use voidmc::components::{Position, Rotation};
use voidmc_protocol::clientbound::{ClientboundPacket, SetTitleText, SetTitlesAnimation};

use crate::art;
use crate::pointer::{ease, look_direction};

/// How long the veil takes to come and to go, in ticks.
pub const VEIL_FADE: u32 = 5;
/// Longer than anything has to hide. The veil is lifted by hand long before.
const VEIL_STAY: u32 = 400;
pub const RISE_TICKS: u32 = 20;
pub const LANDING_TICKS: u32 = 18;
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

/// A camera on its way somewhere. It gets told where to be a segment ahead and the client
/// glides it there by itself, so that a whole flight takes a handful of packets.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Flight {
    from: [f64; 3],
    to: [f64; 3],
    /// The same all the way. It takes a camera that only a flight of display entities can
    /// turn smoothly, and the one that sees for an eye rides on it and cannot.
    look: [f32; 2],
    length: u32,
    ticks: u32,
}

impl Flight {
    pub fn new(from: [f64; 3], to: [f64; 3], look: [f32; 2], length: u32) -> Self {
        Self {
            from,
            to,
            look,
            length,
            ticks: 0,
        }
    }

    /// Straight up from an eye.
    pub fn rise(eye: Position, look: Rotation) -> Self {
        let [x, y, z] = [eye.x, eye.y, eye.z];
        let up = [x, y + FLIGHT_HEIGHT, z];
        Self::new([x, y, z], up, [look.yaw, look.pitch], RISE_TICKS)
    }

    /// The way back, down to an eye.
    pub fn landing(eye: Position, look: Rotation) -> Self {
        Self {
            length: LANDING_TICKS,
            ..Self::rise(eye, look).reversed()
        }
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
            from: self.to,
            to: self.from,
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
        self.to
    }

    fn pose(&self, ticks: u32) -> [f64; 3] {
        let progress = ease(f64::from(ticks.min(self.length)) / f64::from(self.length));
        std::array::from_fn(|axis| self.from[axis] + (self.to[axis] - self.from[axis]) * progress)
    }

    /// Where the view is now.
    pub fn at(&self) -> [f64; 3] {
        self.pose(self.ticks)
    }

    /// How the view looks, as yaw and pitch.
    pub fn looking(&self) -> [f32; 2] {
        self.look
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
        let flight = Flight::rise(EYE, LOOK);
        assert_eq!(flight.at(), [EYE.x, EYE.y, EYE.z]);
        assert_eq!(flight.looking(), [LOOK.yaw, LOOK.pitch]);
    }

    #[test]
    fn a_landing_ends_exactly_where_the_player_sees() {
        let mut flight = Flight::landing(EYE, LOOK);
        while flight.left() > 0 {
            flight.tick();
        }
        assert_eq!(flight.at(), [EYE.x, EYE.y, EYE.z]);
        assert_eq!(flight.target(), flight.end());
        assert_eq!(flight.looking(), [LOOK.yaw, LOOK.pitch]);
    }

    /// A camera that had not stopped would jump when the eye takes over from it.
    #[test]
    fn a_camera_has_stopped_before_the_flight_ends() {
        let mut flight = Flight::rise(EYE, LOOK);
        while !flight.settling() {
            flight.tick();
        }
        assert!(flight.left() > u32::from(FLIGHT_GLIDE_TICKS));
    }

    #[test]
    fn a_flight_waits_for_the_view_to_come_over() {
        let mut flight = Flight::rise(EYE, LOOK);
        for _ in 0..SWAP_TICKS - 1 {
            flight.tick();
            assert_eq!(flight.target(), [EYE.x, EYE.y, EYE.z]);
        }
    }

    /// One packet per segment is the whole point of this.
    #[test]
    fn a_flight_changes_its_target_once_a_segment() {
        let mut flight = Flight::rise(EYE, LOOK);
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
