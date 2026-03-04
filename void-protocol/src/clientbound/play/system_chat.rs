use ussr_nbt::owned::Nbt;
use void_codec::{Decode, Encode};

/// Clientbound System Chat Message (0x73).
/// Sends a system message (not player chat) to the client.
#[derive(Debug, Clone, Encode, Decode)]
pub struct SystemChat {
    pub content: Nbt,
    pub overlay: bool,
}
