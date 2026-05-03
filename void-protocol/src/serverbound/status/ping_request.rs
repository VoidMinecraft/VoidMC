use voidmc_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct PingRequest {
    pub timestamp: i64,
}
