use bevy_ecs::event::Event;
use void_codec::{Decode, Encode};

/// Serverbound Chat Command variant (0x05).
/// We only parse the command string; remaining fields are ignored.
#[derive(Debug, Encode, Decode, Event)]
pub struct ChatCommandUnsigned {
    pub command: String,
    #[codec(remaining)]
    pub _remaining: Vec<u8>,
}
