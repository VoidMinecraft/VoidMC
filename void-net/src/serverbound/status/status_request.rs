use crate::codec::{PacketDecode, PacketEncode};
use crate::{Packet, PacketId};

#[derive(Debug)]
pub struct StatusRequest {}

impl Packet for StatusRequest {
    fn encode<E: PacketEncode>(&self, _encoder: &mut E) -> std::io::Result<()> {
        Ok(())
    }

    fn decode<D: PacketDecode>(_decoder: &mut D) -> std::io::Result<Self> {
        Ok(Self {})
    }
}

impl PacketId for StatusRequest {
    const ID: i32 = 0x00;
}
