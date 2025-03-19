use crate::codec::{PacketDecode, PacketEncode};
use crate::{Packet, PacketId};

#[derive(Debug)]
pub struct GameEvent {
    pub event: u8,
    pub value: f32,
}

impl Packet for GameEvent {
    fn encode<E: PacketEncode>(&self, encoder: &mut E) -> std::io::Result<()> {
        encoder.encode_u8(self.event)?;
        encoder.encode_f32(self.value)
    }

    fn decode<D: PacketDecode>(decoder: &mut D) -> std::io::Result<Self> {
        let event = decoder.decode_u8()?;
        let value = decoder.decode_f32()?;
        Ok(Self { event, value })
    }
}

impl PacketId for GameEvent {
    const ID: i32 = 0x23;
}
