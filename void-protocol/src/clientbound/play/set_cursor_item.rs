use voidmc_codec::{Decode, Encode};

use crate::slot::Slot;

/// Sets the item currently held on the player's cursor (`CLIENTBOUND_SET_CURSOR_ITEM`).
#[derive(Debug, Clone, Encode, Decode)]
pub struct SetCursorItem {
    pub contents: Slot,
}
