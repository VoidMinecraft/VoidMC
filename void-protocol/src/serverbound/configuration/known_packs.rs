use bevy_ecs::event::Event;
use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct KnownPack {
    pub namespace: String,
    pub id: String,
    pub version: String,
}

#[derive(Debug, Encode, Decode, Event)]
pub struct KnownPacks {
    pub known_packs: Vec<KnownPack>,
}
