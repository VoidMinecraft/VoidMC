use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct Ping {
    pub id: i32,
}
