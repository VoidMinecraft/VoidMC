use voidmc_codec::{Decode, Encode};

/// Clientbound Player Rotation: turns the player without moving them. The
/// client answers with its new rotation.
#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct PlayerRotation {
    pub yaw: f32,
    pub relative_yaw: bool,
    pub pitch: f32,
    pub relative_pitch: bool,
}
