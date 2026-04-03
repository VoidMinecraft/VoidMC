use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct KeepAlive {
    pub keep_alive_id: i64,
}
