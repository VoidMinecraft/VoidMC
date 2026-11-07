use voidmc_codec::{Decode, Encode};

/// Clientbound Set Camera: renders the game from another entity. The player
/// keeps receiving input, so its rotation goes on without moving the view.
#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct SetCamera {
    #[codec(varint32)]
    pub camera_id: i32,
}
