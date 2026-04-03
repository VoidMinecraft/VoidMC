use void_codec::{Decode, Encode};

use crate::types::PlayerCommandAction;

#[derive(Debug, Encode, Decode)]
pub struct PlayerCommand {
    #[codec(varint32)]
    pub entity_id: i32,
    pub action_id: PlayerCommandAction,
    #[codec(varint32)]
    pub jump_boost: i32,
}
