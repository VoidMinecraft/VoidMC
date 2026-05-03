use voidmc_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct SetHeldItem {
    pub slot: i16,
}
