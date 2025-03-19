use crate::codec::{PacketDecode, PacketEncode};
use crate::{Packet, PacketId};

#[derive(Debug)]
pub struct SetPlayerPos {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub flags: u8,
}

impl Packet for SetPlayerPos {
    fn encode<E: PacketEncode>(&self, encoder: &mut E) -> std::io::Result<()> {
        encoder.encode_f64(self.x)?;
        encoder.encode_f64(self.y)?;
        encoder.encode_f64(self.z)?;
        encoder.encode_u8(self.flags)
    }

    fn decode<D: PacketDecode>(decoder: &mut D) -> std::io::Result<Self> {
        let x = decoder.decode_f64()?;
        let y = decoder.decode_f64()?;
        let z = decoder.decode_f64()?;
        let flags = decoder.decode_u8()?;

        Ok(Self { x, y, z, flags })
    }
}

impl PacketId for SetPlayerPos {
    const ID: i32 = 0x1C;
}
