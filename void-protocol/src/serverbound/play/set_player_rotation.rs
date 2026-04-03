use bevy_ecs::event::Event;
use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode, Event)]
pub struct SetPlayerRotation {
    pub yaw: f32,
    pub pitch: f32,
    pub flags: u8,
}
