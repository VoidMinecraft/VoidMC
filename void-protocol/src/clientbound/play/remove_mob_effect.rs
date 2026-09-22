use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct RemoveMobEffect {
    #[codec(varint32)]
    pub entity_id: i32,
    #[codec(varint32)]
    pub effect_id: i32,
}

#[cfg(test)]
mod tests {
    use voidmc_data::Version;

    use super::*;
    use crate::clientbound::PlayPacket;

    #[test]
    fn matches_paper_layout() {
        let slowness = voidmc_data::protocol_registry_index(
            Version::V26_1_2,
            "minecraft:mob_effect",
            "minecraft:slowness",
        )
        .unwrap();
        let mut buf = Vec::new();
        PlayPacket::RemoveMobEffect(RemoveMobEffect {
            entity_id: 300,
            effect_id: slowness,
        })
        .encode(&mut buf);
        assert_eq!(buf, [0x4E, 0xAC, 0x02, slowness as u8]);

        let mut slice = &buf[1..];
        let decoded = RemoveMobEffect::decode(&mut slice).unwrap();
        assert!(slice.is_empty());
        assert_eq!(
            decoded,
            RemoveMobEffect {
                entity_id: 300,
                effect_id: slowness
            }
        );
    }
}
