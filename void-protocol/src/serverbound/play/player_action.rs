use void_codec::{Decode, Encode};

use crate::types::{BlockFace, BlockPosition, PlayerActionStatus};

#[derive(Debug, Encode, Decode)]
pub struct PlayerAction {
    pub status: PlayerActionStatus,
    pub position: BlockPosition,
    pub face: BlockFace,
    #[codec(varint32)]
    pub sequence: i32,
}
