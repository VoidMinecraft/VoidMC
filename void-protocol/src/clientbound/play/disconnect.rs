use ussr_nbt::owned::Nbt;
use void_codec::{Decode, Encode};

/// Clientbound Disconnect (Play) — 0x1D. Kicks the client with a reason.
#[derive(Debug, Clone, Encode, Decode)]
pub struct Disconnect {
    pub reason: Nbt,
}
