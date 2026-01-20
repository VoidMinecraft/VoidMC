use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct GameEvent {
    pub event: u8,
    pub value: f32,
}
