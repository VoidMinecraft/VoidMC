use ussr_nbt::owned::Nbt;
use voidmc_codec::{Decode, Encode};

use crate::block_entity::BlockEntityKind;
use crate::types::BlockPosition;

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct BlockEntityData {
    pub position: BlockPosition,
    pub kind: BlockEntityKind,
    pub data: Nbt,
}
