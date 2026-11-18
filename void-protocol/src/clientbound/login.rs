mod login_success;
mod set_compression;

pub use login_success::*;
pub use set_compression::*;
use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
#[codec(tagged, wrap = crate::clientbound::ClientboundPacket::Login)]
pub enum LoginPacket {
    #[codec(packet_id = 0x02)]
    LoginSuccess(LoginSuccess),
    #[codec(packet_id = 0x03)]
    SetCompression(SetCompression),
}
