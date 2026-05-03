use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct KeepAlive {
    pub keep_alive_id: i64,
}
