mod login_success;

pub use login_success::*;
use voidmc_codec::Encode;

#[derive(Debug, Clone, Encode)]
#[codec(tagged, wrap = crate::clientbound::ClientboundPacket::Login)]
pub enum LoginPacket {
    #[codec(packet_id = 0x02)]
    LoginSuccess(LoginSuccess),
}
