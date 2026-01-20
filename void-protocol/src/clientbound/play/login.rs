use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct LastDeathLocation {
    pub dimension: String,
    pub position: u64,
}

#[derive(Debug, Encode, Decode)]
pub struct Login {
    pub entity_id: i32,
    pub is_hardcore: bool,
    pub dimension_names: Vec<String>,
    #[codec(varint32)]
    pub max_players: i32,
    #[codec(varint32)]
    pub view_distance: i32,
    #[codec(varint32)]
    pub simulation_distance: i32,
    pub reduced_debug_info: bool,
    pub enable_respawn_screen: bool,
    pub do_limited_crafting: bool,
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
    pub enforces_secure_chat: bool,
}
