use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct BlockChangedAck {
    #[codec(varint32)]
    pub sequence: i32,
}
