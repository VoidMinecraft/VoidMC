use crate::{Packet, PacketDecode, PacketEncode, PacketId};
use uuid::Uuid;

#[derive(Debug)]
pub struct LoginStart {
    pub name: String,
    pub uuid: Uuid,
}

impl Packet for LoginStart {
    fn encode<E: PacketEncode>(&self, encoder: &mut E) -> std::io::Result<()> {
        encoder.encode_str(&self.name)?;
        encoder.encode_uuid(self.uuid)?;

        Ok(())
    }

    fn decode<D: PacketDecode>(decoder: &mut D) -> std::io::Result<Self> {
        let name = decoder.decode_str()?;
        let uuid = decoder.decode_uuid()?;

        Ok(Self {
            name,
            uuid,
        })
    }
}

impl PacketId for LoginStart {
    const ID: i32 = 0x00;
}