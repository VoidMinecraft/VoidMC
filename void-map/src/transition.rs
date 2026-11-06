//! The way between the world and the map. The camera rises out of the world into a veil
//! of cloud, which hides the swap of chunks, and comes back down the same way.

use bevy_ecs::prelude::Component;
use voidmc::components::{Position, Rotation};
use voidmc_protocol::clientbound::{ClientboundPacket, SetTitleText, SetTitlesAnimation};

use crate::art;
use crate::pointer::{DIVE_PITCH, ease, shortest_turn};

/// How long the veil takes to come and to go, in ticks.
pub const VEIL_FADE: u32 = 8;
/// Longer than anything has to hide. The veil is lifted by hand long before.
const VEIL_STAY: u32 = 400;
pub const RISE_TICKS: u32 = 24;
pub const LANDING_TICKS: u32 = 22;
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

/// A camera on its way between the eye of a player and the sky over it.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Flight {
    from: [f64; 3],
    to: [f64; 3],
    look_from: [f32; 2],
    look_to: [f32; 2],
    length: u32,
    ticks: u32,
}

impl Flight {
    /// Up from an eye, turning to look down on the player.
    pub fn rise(eye: Position, look: Rotation) -> Self {
        let [x, y, z] = [eye.x, eye.y, eye.z];
        Self {
            from: [x, y, z],
            to: [x, y + FLIGHT_HEIGHT, z],
            look_from: [look.yaw, look.pitch],
            look_to: [look.yaw, DIVE_PITCH],
            length: RISE_TICKS,
            ticks: 0,
        }
    }

    /// The way back, down to an eye.
    pub fn landing(eye: Position, look: Rotation) -> Self {
        let up = Self::rise(eye, look);
        Self {
            from: up.to,
            to: up.from,
            look_from: up.look_to,
            look_to: up.look_from,
            length: LANDING_TICKS,
            ticks: 0,
        }
    }

    pub fn tick(&mut self) {
        self.ticks = (self.ticks + 1).min(self.length);
    }

    fn progress(&self) -> f64 {
        ease(f64::from(self.ticks) / f64::from(self.length))
    }

    pub fn at(&self) -> Position {
        let progress = self.progress();
        let [x, y, z] = std::array::from_fn(|axis| {
            self.from[axis] + (self.to[axis] - self.from[axis]) * progress
        });
        Position { x, y, z }
    }

    pub fn looking(&self) -> Rotation {
        let progress = self.progress() as f32;
        let [yaw, pitch] = self.look_from;
        Rotation {
            yaw: yaw + shortest_turn(yaw, self.look_to[0]) * progress,
            pitch: pitch + (self.look_to[1] - pitch) * progress,
        }
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
        assert_eq!(flight.at(), EYE);
        assert_eq!(flight.looking(), LOOK);
    }

    #[test]
    fn a_landing_ends_exactly_where_the_player_sees() {
        let mut flight = Flight::landing(EYE, LOOK);
        for _ in 0..LANDING_TICKS {
            flight.tick();
        }
        assert_eq!(flight.at(), EYE);
        assert_eq!(flight.looking(), LOOK);
    }

    #[test]
    fn the_veil_is_whole_before_the_camera_stops() {
        const { assert!(VEIL_FADE < RISE_TICKS && VEIL_FADE < LANDING_TICKS) };
    }
}
