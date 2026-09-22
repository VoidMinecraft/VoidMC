use bitflags::bitflags;
use ussr_nbt::owned::Nbt;
use voidmc_codec::{Decode, DecodeError, Decoder, Encode};

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct SetPlayerTeam {
    pub name: String,
    pub action: TeamAction,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TeamAction {
    Create {
        parameters: TeamParameters,
        entities: Vec<String>,
    },
    Remove,
    Update(TeamParameters),
    AddEntities(Vec<String>),
    RemoveEntities(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct TeamParameters {
    pub display_name: Nbt,
    pub flags: TeamFlags,
    pub name_tag_visibility: NameTagVisibility,
    pub collision_rule: CollisionRule,
    pub color: TeamColor,
    pub prefix: Nbt,
    pub suffix: Nbt,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct TeamFlags: u8 {
        const FRIENDLY_FIRE = 0x01;
        const SEE_INVISIBLE_FRIENDS = 0x02;
    }
}

impl Encode for TeamFlags {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.bits().encode(buf);
    }
}

impl Decode for TeamFlags {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        Ok(TeamFlags::from_bits_truncate(decoder.decode::<u8>()?))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Encode, Decode)]
#[codec(varint32)]
#[repr(i32)]
pub enum NameTagVisibility {
    #[default]
    Always = 0,
    Never = 1,
    HideForOtherTeams = 2,
    HideForOwnTeam = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Encode, Decode)]
#[codec(varint32)]
#[repr(i32)]
pub enum CollisionRule {
    #[default]
    Always = 0,
    Never = 1,
    PushOtherTeams = 2,
    PushOwnTeam = 3,
}

/// `ChatFormatting` ordinals; `Reset` leaves member names uncoloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Encode, Decode)]
#[codec(varint32)]
#[repr(i32)]
pub enum TeamColor {
    Black = 0,
    DarkBlue = 1,
    DarkGreen = 2,
    DarkAqua = 3,
    DarkRed = 4,
    DarkPurple = 5,
    Gold = 6,
    Gray = 7,
    DarkGray = 8,
    Blue = 9,
    Green = 10,
    Aqua = 11,
    Red = 12,
    LightPurple = 13,
    Yellow = 14,
    White = 15,
    Obfuscated = 16,
    Bold = 17,
    Strikethrough = 18,
    Underline = 19,
    Italic = 20,
    #[default]
    Reset = 21,
}

impl Encode for TeamAction {
    fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            TeamAction::Create {
                parameters,
                entities,
            } => {
                0u8.encode(buf);
                parameters.encode(buf);
                entities.encode(buf);
            }
            TeamAction::Remove => 1u8.encode(buf),
            TeamAction::Update(parameters) => {
                2u8.encode(buf);
                parameters.encode(buf);
            }
            TeamAction::AddEntities(entities) => {
                3u8.encode(buf);
                entities.encode(buf);
            }
            TeamAction::RemoveEntities(entities) => {
                4u8.encode(buf);
                entities.encode(buf);
            }
        }
    }
}

impl Decode for TeamAction {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        Ok(match decoder.decode::<u8>()? {
            0 => TeamAction::Create {
                parameters: decoder.decode()?,
                entities: decoder.decode()?,
            },
            1 => TeamAction::Remove,
            2 => TeamAction::Update(decoder.decode()?),
            3 => TeamAction::AddEntities(decoder.decode()?),
            4 => TeamAction::RemoveEntities(decoder.decode()?),
            other => return Err(DecodeError::InvalidPacketId(Some(other))),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::number_format::{string, text_nbt, text_nbt_bytes};
    use super::*;
    use crate::clientbound::PlayPacket;

    fn parameters() -> TeamParameters {
        TeamParameters {
            display_name: text_nbt("Red"),
            flags: TeamFlags::FRIENDLY_FIRE | TeamFlags::SEE_INVISIBLE_FRIENDS,
            name_tag_visibility: NameTagVisibility::HideForOtherTeams,
            collision_rule: CollisionRule::Never,
            color: TeamColor::Red,
            prefix: text_nbt("[R] "),
            suffix: text_nbt(""),
        }
    }

    fn parameter_bytes() -> Vec<u8> {
        let mut bytes = text_nbt_bytes("Red");
        bytes.extend([0x03, 0x02, 0x01, 0x0C]);
        bytes.extend(text_nbt_bytes("[R] "));
        bytes.extend(text_nbt_bytes(""));
        bytes
    }

    fn encode(action: TeamAction) -> Vec<u8> {
        let mut bytes = Vec::new();
        PlayPacket::SetPlayerTeam(SetPlayerTeam {
            name: "red".into(),
            action,
        })
        .encode(&mut bytes);
        bytes
    }

    fn header(method: u8) -> Vec<u8> {
        let mut bytes = vec![0x6D];
        bytes.extend(string("red"));
        bytes.push(method);
        bytes
    }

    fn names(names: &[&str]) -> Vec<u8> {
        let mut bytes = vec![names.len() as u8];
        for name in names {
            bytes.extend(string(name));
        }
        bytes
    }

    #[test]
    fn create_writes_parameters_then_entity_list() {
        let bytes = encode(TeamAction::Create {
            parameters: parameters(),
            entities: vec!["Leo".into(), "Adam".into()],
        });
        let mut expected = header(0);
        expected.extend(parameter_bytes());
        expected.extend(names(&["Leo", "Adam"]));
        assert_eq!(bytes, expected);
    }

    #[test]
    fn remove_is_name_and_method_only() {
        assert_eq!(encode(TeamAction::Remove), header(1));
    }

    #[test]
    fn update_writes_parameters_without_entities() {
        let mut expected = header(2);
        expected.extend(parameter_bytes());
        assert_eq!(encode(TeamAction::Update(parameters())), expected);
    }

    #[test]
    fn membership_methods_write_only_the_entity_list() {
        let mut expected = header(3);
        expected.extend(names(&["Leo"]));
        assert_eq!(
            encode(TeamAction::AddEntities(vec!["Leo".into()])),
            expected
        );

        let mut expected = header(4);
        expected.extend(names(&[]));
        assert_eq!(encode(TeamAction::RemoveEntities(vec![])), expected);
    }

    #[test]
    fn colour_ordinals_match_chat_formatting() {
        let mut bytes = Vec::new();
        TeamColor::Reset.encode(&mut bytes);
        assert_eq!(bytes, [21]);
        let mut bytes = Vec::new();
        TeamColor::Black.encode(&mut bytes);
        assert_eq!(bytes, [0]);
        let mut bytes = Vec::new();
        TeamColor::Italic.encode(&mut bytes);
        assert_eq!(bytes, [20]);
    }

    #[test]
    fn every_action_round_trips_and_unknown_methods_fail() {
        for action in [
            TeamAction::Create {
                parameters: parameters(),
                entities: vec!["a".into()],
            },
            TeamAction::Remove,
            TeamAction::Update(TeamParameters {
                flags: TeamFlags::empty(),
                color: TeamColor::Reset,
                ..parameters()
            }),
            TeamAction::AddEntities(vec!["b".into(), "c".into()]),
            TeamAction::RemoveEntities(vec![]),
        ] {
            let packet = SetPlayerTeam {
                name: "t".into(),
                action,
            };
            let mut bytes = Vec::new();
            packet.encode(&mut bytes);
            let mut slice = bytes.as_slice();
            assert_eq!(SetPlayerTeam::decode(&mut slice).unwrap(), packet);
            assert!(slice.is_empty());
        }
        let mut bytes = string("t");
        bytes.push(5);
        assert!(SetPlayerTeam::decode(&mut bytes.as_slice()).is_err());
    }
}
