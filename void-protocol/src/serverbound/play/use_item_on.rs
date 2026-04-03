use void_codec::{Decode, Encode};

use crate::types::{BlockFace, BlockPosition, Hand};

#[derive(Debug, Encode, Decode)]
pub struct UseItemOn {
    pub hand: Hand,
    pub location: BlockPosition,
    pub face: BlockFace,
    pub cursor_x: f32,
    pub cursor_y: f32,
    pub cursor_z: f32,
    pub inside_block: bool,
    #[codec(varint32)]
    pub sequence: i32,
}
