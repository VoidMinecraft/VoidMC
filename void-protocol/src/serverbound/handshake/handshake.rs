use voidmc_codec::{Decode, Encode};

use crate::State;

#[derive(Debug, Encode, Decode)]
pub struct Handshake {
    #[codec(varint32)]
    pub protocol_version: i32,
    pub server_address: String,
    pub server_port: u16,
    pub next_state: State,
}
