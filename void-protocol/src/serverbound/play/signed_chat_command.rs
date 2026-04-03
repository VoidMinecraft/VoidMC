use void_codec::{Decode, Encode};

/// Serverbound Signed Chat Command (0x05) — signed slash command.
/// We only parse the command string; the remaining crypto fields are ignored.
#[derive(Debug, Encode, Decode)]
pub struct SignedChatCommand {
    pub command: String,
    #[codec(remaining)]
    pub _remaining: Vec<u8>,
}
