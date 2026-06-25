use voidmc_codec::{Decode, Encode};

use crate::slot::Slot;

/// Sent by a creative-mode client to set the contents of an inventory slot
/// (`SERVERBOUND_SET_CREATIVE_MODE_SLOT`).
///
/// `slot` indexes the player window (0..46), or `-1` when dropping into the
/// world. Note: the vanilla codec is the *untrusted* (length-delimited
/// component) form; this decodes component-less items correctly (the common
/// case of grabbing a plain block) and drops items carrying components until the
/// delimited path lands.
#[derive(Debug, Encode, Decode)]
pub struct SetCreativeModeSlot {
    pub slot: i16,
    pub item: Slot,
}
