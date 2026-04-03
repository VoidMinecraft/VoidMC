use bevy_ecs::event::Event;
use void_codec::{Decode, Encode};

use crate::types::Hand;

#[derive(Debug, Encode, Decode, Event)]
pub struct SwingArm {
    pub hand: Hand,
}
