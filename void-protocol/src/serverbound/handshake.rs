mod handshake;

pub use handshake::*;
use voidmc_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
#[codec(tagged)]
pub enum HandshakePacket {
    #[codec(packet_id = 0x00)]
    Handshake(Handshake),
}
