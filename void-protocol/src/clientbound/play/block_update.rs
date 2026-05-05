use voidmc_codec::{Decode, Encode};

use crate::types::BlockPosition;

#[derive(Debug, Clone, Encode, Decode)]
pub struct BlockUpdate {
    pub position: BlockPosition,
    #[codec(varint32)]
    pub block_state_id: i32,
}
