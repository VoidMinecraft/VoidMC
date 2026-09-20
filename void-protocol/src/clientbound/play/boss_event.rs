use bitflags::bitflags;
use ussr_nbt::owned::Nbt;
use uuid::Uuid;
use voidmc_codec::{Decode, DecodeError, Encode, VarI32};

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct BossEvent {
    pub id: Uuid,
    pub action: BossEventAction,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BossEventAction {
    Add {
        title: Nbt,
        progress: f32,
        color: BossBarColor,
        division: BossBarDivision,
        flags: BossBarFlags,
    },
    Remove,
    UpdateProgress(f32),
    UpdateTitle(Nbt),
    UpdateStyle {
        color: BossBarColor,
        division: BossBarDivision,
    },
    UpdateFlags(BossBarFlags),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
#[codec(varint32)]
#[repr(i32)]
pub enum BossBarColor {
    #[default]
    Pink = 0,
    Blue = 1,
    Red = 2,
    Green = 3,
    Yellow = 4,
    Purple = 5,
    White = 6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
#[codec(varint32)]
#[repr(i32)]
pub enum BossBarDivision {
    #[default]
    None = 0,
    Notches6 = 1,
    Notches10 = 2,
    Notches12 = 3,
    Notches20 = 4,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct BossBarFlags: u8 {
        const DARKEN_SCREEN = 0x01;
        const BOSS_MUSIC = 0x02;
        const WORLD_FOG = 0x04;
    }
}

impl Encode for BossBarFlags {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.bits().encode(buf);
    }
}

impl Decode for BossBarFlags {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        Ok(BossBarFlags::from_bits_truncate(u8::decode(buf)?))
    }
}

impl Encode for BossEventAction {
    fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            BossEventAction::Add {
                title,
                progress,
                color,
                division,
                flags,
            } => {
                VarI32(0).encode(buf);
                title.encode(buf);
                progress.encode(buf);
                color.encode(buf);
                division.encode(buf);
                flags.encode(buf);
            }
            BossEventAction::Remove => VarI32(1).encode(buf),
            BossEventAction::UpdateProgress(progress) => {
                VarI32(2).encode(buf);
                progress.encode(buf);
            }
            BossEventAction::UpdateTitle(title) => {
                VarI32(3).encode(buf);
                title.encode(buf);
            }
            BossEventAction::UpdateStyle { color, division } => {
                VarI32(4).encode(buf);
                color.encode(buf);
                division.encode(buf);
            }
            BossEventAction::UpdateFlags(flags) => {
                VarI32(5).encode(buf);
                flags.encode(buf);
            }
        }
    }
}

impl Decode for BossEventAction {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        Ok(match VarI32::decode(buf)?.0 {
            0 => BossEventAction::Add {
                title: Nbt::decode(buf)?,
                progress: f32::decode(buf)?,
                color: BossBarColor::decode(buf)?,
                division: BossBarDivision::decode(buf)?,
                flags: BossBarFlags::decode(buf)?,
            },
            1 => BossEventAction::Remove,
            2 => BossEventAction::UpdateProgress(f32::decode(buf)?),
            3 => BossEventAction::UpdateTitle(Nbt::decode(buf)?),
            4 => BossEventAction::UpdateStyle {
                color: BossBarColor::decode(buf)?,
                division: BossBarDivision::decode(buf)?,
            },
            5 => BossEventAction::UpdateFlags(BossBarFlags::decode(buf)?),
            _ => return Err(DecodeError::InvalidPacketId(None)),
        })
    }
}

#[cfg(test)]
mod tests {
    use ussr_nbt::owned::Tag;

    use super::*;
    use crate::clientbound::PlayPacket;

    const ID: Uuid = Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef);

    fn title(text: &str) -> Nbt {
        Nbt {
            name: "".into(),
            compound: vec![("text".into(), Tag::String(text.into()))].into(),
        }
    }

    fn title_bytes(text: &str) -> Vec<u8> {
        let mut bytes = vec![0x0A, 0x08, 0x00, 0x04];
        bytes.extend(b"text");
        bytes.extend((text.len() as u16).to_be_bytes());
        bytes.extend(text.as_bytes());
        bytes.push(0x00);
        bytes
    }

    fn encode(action: BossEventAction) -> Vec<u8> {
        let mut bytes = Vec::new();
        PlayPacket::BossEvent(BossEvent { id: ID, action }).encode(&mut bytes);
        bytes
    }

    fn header(action: u8) -> Vec<u8> {
        let mut bytes = vec![0x09];
        bytes.extend(ID.as_bytes());
        bytes.push(action);
        bytes
    }

    #[test]
    fn add_encodes_every_field_in_spec_order() {
        let bytes = encode(BossEventAction::Add {
            title: title("Boss"),
            progress: 0.5,
            color: BossBarColor::Red,
            division: BossBarDivision::Notches10,
            flags: BossBarFlags::DARKEN_SCREEN | BossBarFlags::WORLD_FOG,
        });
        let mut expected = header(0);
        expected.extend(title_bytes("Boss"));
        expected.extend(0.5f32.to_be_bytes());
        expected.extend([2, 2, 0x05]);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn update_actions_have_exact_wire_layouts() {
        assert_eq!(encode(BossEventAction::Remove), header(1));

        let mut expected = header(2);
        expected.extend(1.0f32.to_be_bytes());
        assert_eq!(encode(BossEventAction::UpdateProgress(1.0)), expected);

        let mut expected = header(3);
        expected.extend(title_bytes("New"));
        assert_eq!(encode(BossEventAction::UpdateTitle(title("New"))), expected);

        let mut expected = header(4);
        expected.extend([6, 4]);
        assert_eq!(
            encode(BossEventAction::UpdateStyle {
                color: BossBarColor::White,
                division: BossBarDivision::Notches20,
            }),
            expected
        );

        let mut expected = header(5);
        expected.push(0x02);
        assert_eq!(
            encode(BossEventAction::UpdateFlags(BossBarFlags::BOSS_MUSIC)),
            expected
        );
    }

    #[test]
    fn actions_without_text_roundtrip() {
        let actions = [
            BossEventAction::Remove,
            BossEventAction::UpdateProgress(0.75),
            BossEventAction::UpdateStyle {
                color: BossBarColor::Green,
                division: BossBarDivision::Notches12,
            },
            BossEventAction::UpdateFlags(BossBarFlags::empty()),
        ];
        for action in actions {
            let packet = BossEvent { id: ID, action };
            let mut bytes = Vec::new();
            packet.encode(&mut bytes);
            let mut slice = bytes.as_slice();
            assert_eq!(BossEvent::decode(&mut slice).unwrap(), packet);
            assert!(slice.is_empty());
        }
    }

    #[test]
    fn title_update_decodes() {
        let packet = BossEvent {
            id: ID,
            action: BossEventAction::UpdateTitle(title("Phase 2")),
        };
        let mut bytes = Vec::new();
        packet.encode(&mut bytes);
        assert_eq!(BossEvent::decode(&mut bytes.as_slice()).unwrap(), packet);
    }

    #[test]
    fn unknown_action_is_rejected() {
        let mut bytes = ID.as_bytes().to_vec();
        bytes.push(6);
        assert!(BossEvent::decode(&mut bytes.as_slice()).is_err());
    }
}
