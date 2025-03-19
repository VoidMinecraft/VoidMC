use crate::codec::{PacketDecode, PacketEncode};
use crate::{Packet, PacketId};

#[derive(Debug)]
pub struct LastDeathLocation {
    pub dimension: String,
    pub position: u64,
}

#[derive(Debug)]
pub struct Login {
    pub entity_id: i32,
    pub is_hardcore: bool,
    pub dimension_names: Vec<String>,
    pub max_players: i32,
    pub view_distance: i32,
    pub simulation_distance: i32,
    pub reduced_debug_info: bool,
    pub enable_respawn_screen: bool,
    pub do_limited_crafting: bool,
    pub dimension_type: i32,
    pub dimension_name: String,
    pub hashed_seed: i64,
    pub game_mode: u8,
    pub previous_game_mode: i8,
    pub is_debug: bool,
    pub is_flat: bool,
    pub last_death_location: Option<LastDeathLocation>,
    pub portal_cooldown: i32,
    pub sea_level: i32,
    pub enforces_secure_chat: bool,
}

impl Packet for Login {
    fn encode<E: PacketEncode>(&self, encoder: &mut E) -> std::io::Result<()> {
        encoder.encode_i32(self.entity_id)?;
        encoder.encode_bool(self.is_hardcore)?;
        encoder.encode_vari32(self.dimension_names.len() as i32)?;
        for dimension_name in &self.dimension_names {
            encoder.encode_str(dimension_name)?;
        }
        encoder.encode_vari32(self.max_players)?;
        encoder.encode_vari32(self.view_distance)?;
        encoder.encode_vari32(self.simulation_distance)?;
        encoder.encode_bool(self.reduced_debug_info)?;
        encoder.encode_bool(self.enable_respawn_screen)?;
        encoder.encode_bool(self.do_limited_crafting)?;
        encoder.encode_vari32(self.dimension_type)?;
        encoder.encode_str(&self.dimension_name)?;
        encoder.encode_i64(self.hashed_seed)?;
        encoder.encode_u8(self.game_mode)?;
        encoder.encode_i8(self.previous_game_mode)?;
        encoder.encode_bool(self.is_debug)?;
        encoder.encode_bool(self.is_flat)?;
        match &self.last_death_location {
            Some(location) => {
                encoder.encode_bool(true)?;
                encoder.encode_str(&location.dimension)?;
                encoder.encode_u64(location.position)?;
            }
            None => {
                encoder.encode_bool(false)?;
            }
        }
        encoder.encode_vari32(self.portal_cooldown)?;
        encoder.encode_vari32(self.sea_level)?;
        encoder.encode_bool(self.enforces_secure_chat)
    }

    fn decode<D: PacketDecode>(decoder: &mut D) -> std::io::Result<Self> {
        let entity_id = decoder.decode_i32()?;
        let is_hardcore = decoder.decode_bool()?;
        let dimension_names_len = decoder.decode_vari32()?;
        let mut dimension_names = Vec::new();
        for _ in 0..dimension_names_len {
            dimension_names.push(decoder.decode_str()?);
        }
        let max_players = decoder.decode_vari32()?;
        let view_distance = decoder.decode_vari32()?;
        let simulation_distance = decoder.decode_vari32()?;
        let reduced_debug_info = decoder.decode_bool()?;
        let enable_respawn_screen = decoder.decode_bool()?;
        let do_limited_crafting = decoder.decode_bool()?;
        let dimension_type = decoder.decode_vari32()?;
        let dimension_name = decoder.decode_str()?;
        let hashed_seed = decoder.decode_i64()?;
        let game_mode = decoder.decode_u8()?;
        let previous_game_mode = decoder.decode_i8()?;
        let is_debug = decoder.decode_bool()?;
        let is_flat = decoder.decode_bool()?;
        let has_death_location = if decoder.decode_bool()? {
            Some(LastDeathLocation {
                dimension: decoder.decode_str()?,
                position: decoder.decode_u64()?,
            })
        } else {
            None
        };
        let portal_cooldown = decoder.decode_vari32()?;
        let sea_level = decoder.decode_vari32()?;
        let enforces_secure_chat = decoder.decode_bool()?;

        Ok(Self {
            entity_id,
            is_hardcore,
            dimension_names,
            max_players,
            view_distance,
            simulation_distance,
            reduced_debug_info,
            enable_respawn_screen,
            do_limited_crafting,
            dimension_type,
            dimension_name,
            hashed_seed,
            game_mode,
            previous_game_mode,
            is_debug,
            is_flat,
            last_death_location: has_death_location,
            portal_cooldown,
            sea_level,
            enforces_secure_chat,
        })
    }
}

impl PacketId for Login {
    const ID: i32 = 0x2C;
}
