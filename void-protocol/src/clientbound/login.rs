mod login_success;

pub use login_success::*;
use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
#[codec(tagged)]
pub enum LoginPacket {
    #[codec(packet_id = 0x02)]
    LoginSuccess(LoginSuccess),
}
