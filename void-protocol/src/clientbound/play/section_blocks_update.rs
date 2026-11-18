use voidmc_codec::{Decode, DecodeError, Decoder, Encode, VarI64};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionBlocksUpdate {
    pub section_x: i32,
    pub section_y: i32,
    pub section_z: i32,
    pub blocks: Vec<SectionBlockChange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionBlockChange {
    pub x: u8,
    pub y: u8,
    pub z: u8,
    pub block_state_id: i32,
}

impl SectionBlocksUpdate {
    pub fn packed_section(&self) -> i64 {
        ((self.section_x as i64 & 0x3F_FFFF) << 42)
            | ((self.section_z as i64 & 0x3F_FFFF) << 20)
            | (self.section_y as i64 & 0xF_FFFF)
    }
}

fn sign_extend(value: i64, bits: u32) -> i32 {
    let shift = 64 - bits;
    ((value << shift) >> shift) as i32
}

impl Encode for SectionBlocksUpdate {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.packed_section().encode(buf);
        self.blocks.encode(buf);
    }
}

impl Decode for SectionBlocksUpdate {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let packed = decoder.decode::<i64>()?;
        Ok(Self {
            section_x: sign_extend(packed >> 42, 22),
            section_y: sign_extend(packed, 20),
            section_z: sign_extend(packed >> 20, 22),
            blocks: decoder.decode()?,
        })
    }
}

impl Encode for SectionBlockChange {
    fn encode(&self, buf: &mut Vec<u8>) {
        let local =
            (i64::from(self.x & 15) << 8) | (i64::from(self.z & 15) << 4) | i64::from(self.y & 15);
        VarI64((i64::from(self.block_state_id) << 12) | local).encode(buf);
    }
}

impl Decode for SectionBlockChange {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let raw = decoder.decode::<VarI64>()?.0;
        Ok(Self {
            x: ((raw >> 8) & 15) as u8,
            y: (raw & 15) as u8,
            z: ((raw >> 4) & 15) as u8,
            block_state_id: (raw >> 12) as i32,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::PlayPacket;

    #[test]
    fn matches_vanilla_layout() {
        let packet = SectionBlocksUpdate {
            section_x: -1,
            section_y: -4,
            section_z: 2,
            blocks: vec![
                SectionBlockChange {
                    x: 15,
                    y: 3,
                    z: 1,
                    block_state_id: 1,
                },
                SectionBlockChange {
                    x: 0,
                    y: 0,
                    z: 0,
                    block_state_id: 29_872,
                },
            ],
        };
        let mut buf = Vec::new();
        PlayPacket::SectionBlocksUpdate(packet.clone()).encode(&mut buf);

        let mut expected = vec![0x54];
        expected.extend_from_slice(&0xFFFF_FC00_002F_FFFC_u64.to_be_bytes());
        expected.push(2);
        expected.extend_from_slice(&[0x93, 0x3E]);
        expected.extend_from_slice(&[0x80, 0x80, 0xAC, 0x3A]);
        assert_eq!(buf, expected);

        let mut slice = &buf[1..];
        let decoded = SectionBlocksUpdate::decode(&mut slice).unwrap();
        assert!(slice.is_empty());
        assert_eq!(decoded, packet);
    }

    #[test]
    fn packs_section_coordinates_like_section_pos_as_long() {
        let packet = SectionBlocksUpdate {
            section_x: 1,
            section_y: 2,
            section_z: 3,
            blocks: Vec::new(),
        };
        assert_eq!(packet.packed_section(), (1 << 42) | (3 << 20) | 2);
    }
}
