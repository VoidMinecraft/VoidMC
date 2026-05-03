use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct KnownPack {
    pub namespace: String,
    pub id: String,
    pub version: String,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct KnownPacks {
    pub known_packs: Vec<KnownPack>,
}
