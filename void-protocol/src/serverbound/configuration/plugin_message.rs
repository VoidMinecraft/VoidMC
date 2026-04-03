use bevy_ecs::event::Event;
use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode, Event)]
pub struct PluginMessage {
    pub channel: String,
    #[codec(remaining)]
    pub data: Vec<u8>,
}
