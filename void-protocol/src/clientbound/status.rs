mod ping_response;
mod status_response;

pub use ping_response::*;
pub use status_response::*;
use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
#[codec(tagged)]
pub enum StatusPacket {
    #[codec(packet_id = 0x00)]
    StatusResponse(StatusResponse),
    #[codec(packet_id = 0x01)]
    PingResponse(PingResponse),
}
