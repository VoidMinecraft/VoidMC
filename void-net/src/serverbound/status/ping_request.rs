use crate::codec::{PacketDecode, PacketEncode};
use crate::{Packet, PacketId};

#[derive(Debug)]
pub struct PingRequest {
    pub timestamp: i64,
}

impl Packet for PingRequest {
    fn encode<E: PacketEncode>(&self, encoder: &mut E) -> std::io::Result<()> {
        encoder.encode_i64(self.timestamp)
    }

    fn decode<D: PacketDecode>(decoder: &mut D) -> std::io::Result<Self> {
        let timestamp = decoder.decode_i64()?;

        Ok(Self { timestamp })
    }
}

impl PacketId for PingRequest {
    const ID: i32 = 0x01;
}
