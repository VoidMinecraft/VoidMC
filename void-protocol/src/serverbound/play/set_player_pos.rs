use bevy_ecs::event::Event;
use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode, Event)]
pub struct SetPlayerPos {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub flags: u8,
}
