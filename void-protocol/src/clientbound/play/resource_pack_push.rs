use ussr_nbt::owned::Nbt;
use uuid::Uuid;
use voidmc_codec::{Decode, Encode};

/// Clientbound Resource Pack Push: asks the client to download and apply a pack.
#[derive(Debug, Clone, Encode, Decode)]
pub struct ResourcePackPush {
    pub id: Uuid,
    pub url: String,
    /// Lowercase hex SHA-1 of the archive; empty skips verification and caching.
    pub hash: String,
    pub required: bool,
    pub prompt: Option<Nbt>,
}
