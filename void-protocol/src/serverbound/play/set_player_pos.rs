use voidmc_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct SetPlayerPos {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub flags: u8,
}
