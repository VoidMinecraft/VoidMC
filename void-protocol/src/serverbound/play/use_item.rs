use voidmc_codec::{Decode, Encode};

use crate::types::Hand;

#[derive(Debug, Encode, Decode)]
pub struct UseItem {
    pub hand: Hand,
    #[codec(varint32)]
    pub sequence: i32,
    pub yaw: f32,
    pub pitch: f32,
}
