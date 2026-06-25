use voidmc_codec::{Decode, Encode};

/// Tells the client which hotbar slot the server considers selected
/// (`CLIENTBOUND_SET_HELD_SLOT`). `slot` is 0..=8.
#[derive(Debug, Clone, Encode, Decode)]
pub struct SetHeldSlot {
    #[codec(varint32)]
    pub slot: i32,
}
