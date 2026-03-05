use void_codec::{Decode, Encode};

/// Serverbound Chat Command (0x04) — unsigned slash command.
/// The client sends this when the player types a / command.
#[derive(Debug, Encode, Decode)]
pub struct ChatCommand {
    pub command: String,
}
