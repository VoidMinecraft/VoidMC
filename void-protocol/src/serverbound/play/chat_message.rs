use voidmc_codec::{Decode, Encode};

/// Serverbound Chat Message (0x06) — regular chat message.
/// We only parse the message string; the remaining crypto fields are ignored.
#[derive(Debug, Encode, Decode)]
pub struct ChatMessage {
    pub message: String,
    #[codec(remaining)]
    pub _remaining: Vec<u8>,
}
