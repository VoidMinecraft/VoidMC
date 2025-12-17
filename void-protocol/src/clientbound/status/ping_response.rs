use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct PingResponse {
    pub timestamp: i64,
}
