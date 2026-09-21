use ussr_nbt::owned::Nbt;
use voidmc_codec::{Decode, DecodeError, Decoder, Encode};

use super::NumberFormat;

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct SetObjective {
    pub name: String,
    pub action: ObjectiveAction,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ObjectiveAction {
    Create(ObjectiveInfo),
    Remove,
    Update(ObjectiveInfo),
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct ObjectiveInfo {
    pub display_name: Nbt,
    pub render_type: RenderType,
    pub number_format: Option<NumberFormat>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
#[codec(varint32)]
#[repr(i32)]
pub enum RenderType {
    #[default]
    Integer = 0,
    Hearts = 1,
}

impl Encode for ObjectiveAction {
    fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            ObjectiveAction::Create(info) => {
                0u8.encode(buf);
                info.encode(buf);
            }
            ObjectiveAction::Remove => 1u8.encode(buf),
            ObjectiveAction::Update(info) => {
                2u8.encode(buf);
                info.encode(buf);
            }
        }
    }
}

impl Decode for ObjectiveAction {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        Ok(match decoder.decode::<u8>()? {
            0 => ObjectiveAction::Create(decoder.decode()?),
            1 => ObjectiveAction::Remove,
            2 => ObjectiveAction::Update(decoder.decode()?),
            other => return Err(DecodeError::InvalidPacketId(Some(other))),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::number_format::{string, text_nbt, text_nbt_bytes};
    use super::*;
    use crate::clientbound::PlayPacket;

    fn encode(action: ObjectiveAction) -> Vec<u8> {
        let mut bytes = Vec::new();
        PlayPacket::SetObjective(SetObjective {
            name: "race".into(),
            action,
        })
        .encode(&mut bytes);
        bytes
    }

    fn header(method: u8) -> Vec<u8> {
        let mut bytes = vec![0x6A];
        bytes.extend(string("race"));
        bytes.push(method);
        bytes
    }

    #[test]
    fn create_writes_name_method_component_render_type_and_format() {
        let bytes = encode(ObjectiveAction::Create(ObjectiveInfo {
            display_name: text_nbt("Alpine Rush"),
            render_type: RenderType::Hearts,
            number_format: None,
        }));
        let mut expected = header(0);
        expected.extend(text_nbt_bytes("Alpine Rush"));
        expected.extend([0x01, 0x00]);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn update_carries_the_optional_number_format() {
        let bytes = encode(ObjectiveAction::Update(ObjectiveInfo {
            display_name: text_nbt("Lap"),
            render_type: RenderType::Integer,
            number_format: Some(NumberFormat::Blank),
        }));
        let mut expected = header(2);
        expected.extend(text_nbt_bytes("Lap"));
        expected.extend([0x00, 0x01, 0x00]);
        assert_eq!(bytes, expected);

        let bytes = encode(ObjectiveAction::Update(ObjectiveInfo {
            display_name: text_nbt("Lap"),
            render_type: RenderType::Integer,
            number_format: Some(NumberFormat::Fixed(text_nbt("-"))),
        }));
        let mut expected = header(2);
        expected.extend(text_nbt_bytes("Lap"));
        expected.extend([0x00, 0x01, 0x02]);
        expected.extend(text_nbt_bytes("-"));
        assert_eq!(bytes, expected);
    }

    #[test]
    fn remove_is_name_and_method_only() {
        assert_eq!(encode(ObjectiveAction::Remove), header(1));
    }

    #[test]
    fn every_action_round_trips_and_unknown_methods_fail() {
        for action in [
            ObjectiveAction::Create(ObjectiveInfo {
                display_name: text_nbt("a"),
                render_type: RenderType::Hearts,
                number_format: Some(NumberFormat::Fixed(text_nbt("x"))),
            }),
            ObjectiveAction::Remove,
            ObjectiveAction::Update(ObjectiveInfo {
                display_name: text_nbt("b"),
                render_type: RenderType::Integer,
                number_format: None,
            }),
        ] {
            let packet = SetObjective {
                name: "o".into(),
                action,
            };
            let mut bytes = Vec::new();
            packet.encode(&mut bytes);
            let mut slice = bytes.as_slice();
            assert_eq!(SetObjective::decode(&mut slice).unwrap(), packet);
            assert!(slice.is_empty());
        }
        let mut bytes = string("o");
        bytes.push(3);
        assert!(SetObjective::decode(&mut bytes.as_slice()).is_err());
    }
}
