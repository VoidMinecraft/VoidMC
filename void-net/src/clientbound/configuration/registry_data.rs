use crate::codec::{PacketDecode, PacketEncode};
use crate::{Packet, PacketId};

use ussr_nbt::owned::Nbt;

#[derive(Debug)]
pub struct RegistryEntry {
    pub entry_id: String,
    pub data: Option<Nbt>,
}

#[derive(Debug)]
pub struct RegistryData {
    pub registry_id: String,
    pub entries: Vec<RegistryEntry>,
}

impl Packet for RegistryData {
    fn encode<E: PacketEncode>(&self, encoder: &mut E) -> std::io::Result<()> {
        encoder.encode_str(&self.registry_id)?;
        encoder.encode_vari32(self.entries.len() as i32)?;

        for entry in &self.entries {
            encoder.encode_str(&entry.entry_id)?;

            match entry.data {
                Some(ref nbt) => {
                    encoder.encode_bool(true)?;
                    encoder.encode_nbt(&nbt)?;
                }
                None => {
                    encoder.encode_bool(false)?;
                }
            }
        }

        Ok(())
    }

    fn decode<D: PacketDecode>(decoder: &mut D) -> std::io::Result<Self> {
        let registry_id = decoder.decode_str()?;
        let entries_len = decoder.decode_vari32()?;

        let mut entries = Vec::new();
        for _ in 0..entries_len {
            let entry_id = decoder.decode_str()?;
            let has_data = decoder.decode_bool()?;
            let data = if has_data {
                Some(decoder.decode_nbt()?)
            } else {
                None
            };

            entries.push(RegistryEntry { entry_id, data });
        }

        Ok(Self {
            registry_id,
            entries,
        })
    }
}

impl PacketId for RegistryData {
    const ID: i32 = 0x07;
}
