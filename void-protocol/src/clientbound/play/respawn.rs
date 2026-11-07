use voidmc_codec::{Decode, Encode};

use super::LastDeathLocation;

/// Clientbound Respawn: moves the player to another level. The client drops its
/// chunks and entities, so the server sends what the new level holds.
#[derive(Debug, Clone, Encode, Decode)]
pub struct Respawn {
    /// Index in the synced `minecraft:dimension_type` registry.
    #[codec(varint32)]
    pub dimension_type: i32,
    pub dimension_name: String,
    pub hashed_seed: i64,
    pub game_mode: u8,
    pub previous_game_mode: i8,
    pub is_debug: bool,
    pub is_flat: bool,
    pub last_death_location: Option<LastDeathLocation>,
    #[codec(varint32)]
    pub portal_cooldown: i32,
    #[codec(varint32)]
    pub sea_level: i32,
    pub data_to_keep: u8,
}

impl Respawn {
    pub const KEEP_ATTRIBUTES: u8 = 0x01;
    pub const KEEP_ENTITY_DATA: u8 = 0x02;
    pub const KEEP_ALL: u8 = Self::KEEP_ATTRIBUTES | Self::KEEP_ENTITY_DATA;
}
