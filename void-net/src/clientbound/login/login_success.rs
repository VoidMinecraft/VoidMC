use crate::codec::{PacketDecode, PacketEncode};
use crate::{Packet, PacketId};
use uuid::Uuid;

#[derive(Debug)]
pub struct Property {
    pub name: String,
    pub value: String,
    pub signature: Option<String>,
}

#[derive(Debug)]
pub struct LoginSuccess {
    pub uuid: Uuid,
    pub username: String,
    pub properties: Vec<Property>,
}

impl Packet for LoginSuccess {
    fn encode<E: PacketEncode>(&self, encoder: &mut E) -> std::io::Result<()> {
        encoder.encode_uuid(self.uuid)?;
        encoder.encode_str(&self.username)?;
        encoder.encode_vari32(self.properties.len() as i32)?;
        for property in &self.properties {
            encoder.encode_str(&property.name)?;
            encoder.encode_str(&property.value)?;
            match &property.signature {
                Some(signature) => {
                    encoder.encode_bool(true)?;
                    encoder.encode_str(&signature)?;
                }
                None => {
                    encoder.encode_bool(false)?;
                }
            }
        }
        Ok(())
    }

    fn decode<D: PacketDecode>(decoder: &mut D) -> std::io::Result<Self> {
        let uuid = decoder.decode_uuid()?;
        let username = decoder.decode_str()?;
        let properties_len = decoder.decode_vari32()?;
        let mut properties = Vec::new();
        for _ in 0..properties_len {
            let name = decoder.decode_str()?;
            let value = decoder.decode_str()?;
            let signature_present = decoder.decode_bool()?;
            let signature = if signature_present {
                Some(decoder.decode_str()?)
            } else {
                None
            };
            properties.push(Property {
                name,
                value,
                signature,
            });
        }
        Ok(Self {
            uuid,
            username,
            properties,
        })
    }
}

impl PacketId for LoginSuccess {
    const ID: i32 = 0x02;
}