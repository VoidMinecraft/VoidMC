use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct PluginMessage {
    pub channel: String,
    #[codec(remaining)]
    pub data: Vec<u8>,
}
