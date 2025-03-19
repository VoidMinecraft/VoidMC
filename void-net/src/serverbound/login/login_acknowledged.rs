use crate::{Packet, PacketDecode, PacketEncode, PacketId};

#[derive(Debug)]
pub struct LoginAcknowledged {}

impl Packet for LoginAcknowledged {
    fn encode<E: PacketEncode>(&self, _encoder: &mut E) -> std::io::Result<()> {
        Ok(())
    }

    fn decode<D: PacketDecode>(_decoder: &mut D) -> std::io::Result<Self> {
        Ok(Self {})
    }
}

impl PacketId for LoginAcknowledged {
    const ID: i32 = 0x03;
}