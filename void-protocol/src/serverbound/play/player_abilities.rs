use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct PlayerAbilities {
    pub flags: u8,
}
