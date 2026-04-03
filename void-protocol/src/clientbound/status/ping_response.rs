use void_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct PingResponse {
    pub timestamp: i64,
}
