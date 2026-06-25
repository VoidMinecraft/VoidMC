use voidmc_codec::{Decode, Encode};

use crate::slot::Slot;

/// Updates a single slot of a container window (`CLIENTBOUND_CONTAINER_SET_SLOT`).
///
/// `container_id` 0 with `slot < 0` targets the cursor / hotbar specials; for the
/// player inventory `slot` indexes the 46-slot window directly.
#[derive(Debug, Clone, Encode, Decode)]
pub struct SetContainerSlot {
    #[codec(varint32)]
    pub container_id: i32,
    #[codec(varint32)]
    pub state_id: i32,
    pub slot: i16,
    pub data: Slot,
}
