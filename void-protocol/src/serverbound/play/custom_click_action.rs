use voidmc_codec::{Decode, Encode};

/// Serverbound Custom Click Action: a `minecraft:custom` click event fired on
/// the client, from a dialog button or a text component.
#[derive(Debug, Encode, Decode)]
pub struct CustomClickAction {
    pub id: String,
    /// Length-prefixed optional NBT tag, left undecoded.
    #[codec(remaining)]
    pub payload: Vec<u8>,
}
