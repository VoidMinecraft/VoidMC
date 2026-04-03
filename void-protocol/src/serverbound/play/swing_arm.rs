use void_codec::{Decode, Encode};

use crate::types::Hand;

#[derive(Debug, Encode, Decode)]
pub struct SwingArm {
    pub hand: Hand,
}
