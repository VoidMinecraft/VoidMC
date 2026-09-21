use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct SetCooldown {
    pub cooldown_group: String,
    #[codec(varint32)]
    pub duration: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::PlayPacket;

    #[test]
    fn matches_paper_layout() {
        let mut buf = Vec::new();
        SetCooldown {
            cooldown_group: "minecraft:ender_pearl".into(),
            duration: 300,
        }
        .encode(&mut buf);
        let mut expected = vec![21u8];
        expected.extend_from_slice(b"minecraft:ender_pearl");
        expected.extend_from_slice(&[0xAC, 0x02]);
        assert_eq!(buf, expected);

        let mut slice = buf.as_slice();
        let decoded = SetCooldown::decode(&mut slice).unwrap();
        assert!(slice.is_empty());
        assert_eq!(decoded.cooldown_group, "minecraft:ender_pearl");
        assert_eq!(decoded.duration, 300);
    }

    #[test]
    fn tagged_packet_id() {
        let mut buf = Vec::new();
        PlayPacket::SetCooldown(SetCooldown {
            cooldown_group: "a".into(),
            duration: 0,
        })
        .encode(&mut buf);
        assert_eq!(buf, vec![0x16, 0x01, b'a', 0x00]);
    }
}
