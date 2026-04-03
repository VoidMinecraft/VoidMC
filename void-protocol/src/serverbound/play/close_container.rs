use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct CloseContainer {
    pub window_id: u8,
}
