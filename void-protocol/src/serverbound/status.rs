mod ping_request;
mod status_request;

pub use ping_request::PingRequest;
pub use status_request::StatusRequest;
use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
#[codec(tagged)]
pub enum StatusPacket {
    #[codec(packet_id = 0x00)]
    StatusRequest(StatusRequest),
    #[codec(packet_id = 0x01)]
    PingRequest(PingRequest),
}
