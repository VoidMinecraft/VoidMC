use bevy_ecs::event::Event;
use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode, Event)]
pub struct CloseContainer {
    pub window_id: u8,
}
