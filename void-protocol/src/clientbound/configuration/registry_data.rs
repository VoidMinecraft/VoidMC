use ussr_nbt::owned::Nbt;
use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct RegistryEntry {
    pub entry_id: String,
    pub data: Option<Nbt>,
}

#[derive(Debug, Encode, Decode)]
pub struct RegistryData {
    pub registry_id: String,
    pub entries: Vec<RegistryEntry>,
}
