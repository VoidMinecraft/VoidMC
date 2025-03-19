use crate::codec::{PacketDecode, PacketEncode};
use crate::{Packet, PacketId};

#[derive(Debug)]
pub struct KnownPack {
    pub namespace: String,
    pub id: String,
    pub version: String,
}

#[derive(Debug)]
pub struct KnownPacks {
    pub known_packs: Vec<KnownPack>,
}

impl Packet for KnownPacks {
    fn encode<E: PacketEncode>(&self, encoder: &mut E) -> std::io::Result<()> {
        encoder.encode_vari32(self.known_packs.len() as i32)?;
        for pack in &self.known_packs {
            encoder.encode_str(&pack.namespace)?;
            encoder.encode_str(&pack.id)?;
            encoder.encode_str(&pack.version)?;
        }
        Ok(())
    }

    fn decode<D: PacketDecode>(decoder: &mut D) -> std::io::Result<Self> {
        let known_packs_len = decoder.decode_vari32()?;
        let mut known_packs = Vec::new();

        for _ in 0..known_packs_len {
            let namespace = decoder.decode_str()?;
            let id = decoder.decode_str()?;
            let version = decoder.decode_str()?;

            known_packs.push(KnownPack {
                namespace,
                id,
                version,
            });
        }

        Ok(Self { known_packs })
    }
}

impl PacketId for KnownPacks {
    const ID: i32 = 0x07;
}
