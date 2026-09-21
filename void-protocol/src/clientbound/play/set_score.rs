use ussr_nbt::owned::Nbt;
use voidmc_codec::{Decode, Encode};

use super::NumberFormat;

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct SetScore {
    pub owner: String,
    pub objective: String,
    #[codec(varint32)]
    pub value: i32,
    pub display_name: Option<Nbt>,
    pub number_format: Option<NumberFormat>,
}

#[cfg(test)]
mod tests {
    use super::super::number_format::{string, text_nbt, text_nbt_bytes};
    use super::*;
    use crate::clientbound::PlayPacket;

    fn encode(packet: SetScore) -> Vec<u8> {
        let mut bytes = Vec::new();
        PlayPacket::SetScore(packet).encode(&mut bytes);
        bytes
    }

    #[test]
    fn bare_score_has_two_absent_optionals() {
        let bytes = encode(SetScore {
            owner: "Leo".into(),
            objective: "race".into(),
            value: 300,
            display_name: None,
            number_format: None,
        });
        let mut expected = vec![0x6E];
        expected.extend(string("Leo"));
        expected.extend(string("race"));
        expected.extend([0xAC, 0x02, 0x00, 0x00]);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn display_name_and_format_follow_the_value() {
        let bytes = encode(SetScore {
            owner: "Leo".into(),
            objective: "race".into(),
            value: -1,
            display_name: Some(text_nbt("Léo")),
            number_format: Some(NumberFormat::Fixed(text_nbt("1st"))),
        });
        let mut expected = vec![0x6E];
        expected.extend(string("Leo"));
        expected.extend(string("race"));
        expected.extend([0xFF, 0xFF, 0xFF, 0xFF, 0x0F, 0x01]);
        expected.extend(text_nbt_bytes("Léo"));
        expected.extend([0x01, 0x02]);
        expected.extend(text_nbt_bytes("1st"));
        assert_eq!(bytes, expected);
    }

    #[test]
    fn round_trips() {
        let packet = SetScore {
            owner: "a".into(),
            objective: "b".into(),
            value: 7,
            display_name: Some(text_nbt("c")),
            number_format: Some(NumberFormat::Blank),
        };
        let mut bytes = Vec::new();
        packet.encode(&mut bytes);
        let mut slice = bytes.as_slice();
        assert_eq!(SetScore::decode(&mut slice).unwrap(), packet);
        assert!(slice.is_empty());
    }
}
