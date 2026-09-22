use bitflags::bitflags;
use voidmc_codec::{Decode, DecodeError, Decoder, Encode};

/// `ClientboundUpdateMobEffectPacket`: adds or refreshes one effect on an
/// entity. `duration` is in ticks, `INFINITE_DURATION` never expires.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct UpdateMobEffect {
    #[codec(varint32)]
    pub entity_id: i32,
    #[codec(varint32)]
    pub effect_id: i32,
    #[codec(varint32)]
    pub amplifier: i32,
    #[codec(varint32)]
    pub duration: i32,
    pub flags: MobEffectFlags,
}

pub const INFINITE_DURATION: i32 = -1;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct MobEffectFlags: u8 {
        const AMBIENT = 0x01;
        const SHOW_PARTICLES = 0x02;
        const SHOW_ICON = 0x04;
        const BLEND = 0x08;
    }
}

impl Encode for MobEffectFlags {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.bits().encode(buf);
    }
}

impl Decode for MobEffectFlags {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        Ok(MobEffectFlags::from_bits_truncate(decoder.decode::<u8>()?))
    }
}

#[cfg(test)]
mod tests {
    use voidmc_data::Version;

    use super::*;
    use crate::clientbound::PlayPacket;

    fn effect_id(name: &str) -> i32 {
        voidmc_data::protocol_registry_index(Version::V26_1_2, "minecraft:mob_effect", name)
            .unwrap()
    }

    #[test]
    fn matches_paper_layout() {
        let mut buf = Vec::new();
        UpdateMobEffect {
            entity_id: 300,
            effect_id: effect_id("minecraft:speed"),
            amplifier: 1,
            duration: 600,
            flags: MobEffectFlags::SHOW_PARTICLES | MobEffectFlags::SHOW_ICON,
        }
        .encode(&mut buf);
        assert_eq!(buf, [0xAC, 0x02, 0x00, 0x01, 0xD8, 0x04, 0x06]);

        let mut slice = buf.as_slice();
        let decoded = UpdateMobEffect::decode(&mut slice).unwrap();
        assert!(slice.is_empty());
        assert_eq!(decoded.entity_id, 300);
        assert_eq!(decoded.amplifier, 1);
        assert_eq!(decoded.duration, 600);
        assert_eq!(
            decoded.flags,
            MobEffectFlags::SHOW_PARTICLES | MobEffectFlags::SHOW_ICON
        );
    }

    #[test]
    fn infinite_duration_is_minus_one_varint_and_flags_bits_match_paper() {
        let mut buf = Vec::new();
        UpdateMobEffect {
            entity_id: 1,
            effect_id: effect_id("minecraft:glowing"),
            amplifier: 0,
            duration: INFINITE_DURATION,
            flags: MobEffectFlags::AMBIENT | MobEffectFlags::BLEND,
        }
        .encode(&mut buf);
        assert_eq!(
            buf,
            [
                0x01,
                effect_id("minecraft:glowing") as u8,
                0x00,
                0xFF,
                0xFF,
                0xFF,
                0xFF,
                0x0F,
                0x09
            ]
        );
    }

    #[test]
    fn tagged_packet_id() {
        let mut buf = Vec::new();
        PlayPacket::UpdateMobEffect(UpdateMobEffect {
            entity_id: 1,
            effect_id: 0,
            amplifier: 0,
            duration: 1,
            flags: MobEffectFlags::empty(),
        })
        .encode(&mut buf);
        assert_eq!(buf, [0x84, 0x01, 0x01, 0x00, 0x00, 0x01, 0x00]);
    }
}
