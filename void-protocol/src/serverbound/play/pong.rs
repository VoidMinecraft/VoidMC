use voidmc_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct Pong {
    pub id: i32,
}
