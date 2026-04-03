mod login_acknowledged;
mod login_start;

pub use login_acknowledged::*;
pub use login_start::*;
use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
#[codec(tagged)]
pub enum LoginPacket {
    #[codec(packet_id = 0x00)]
    LoginStart(LoginStart),
    #[codec(packet_id = 0x03)]
    LoginAcknowledged(LoginAcknowledged),
}
