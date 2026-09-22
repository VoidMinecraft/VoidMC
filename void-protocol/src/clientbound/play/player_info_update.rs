use bitflags::bitflags;
use ussr_nbt::owned::Nbt;
use uuid::Uuid;
use voidmc_codec::{Encode, VarI32};

use crate::clientbound::Property;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct PlayerInfoActions: u8 {
        const ADD_PLAYER = 0x01;
        const INITIALIZE_CHAT = 0x02;
        const UPDATE_GAME_MODE = 0x04;
        const UPDATE_LISTED = 0x08;
        const UPDATE_LATENCY = 0x10;
        const UPDATE_DISPLAY_NAME = 0x20;
        const UPDATE_LIST_ORDER = 0x40;
        const UPDATE_HAT = 0x80;
    }
}

impl PlayerInfoActions {
    pub const INITIALIZING: Self = Self::ADD_PLAYER
        .union(Self::INITIALIZE_CHAT)
        .union(Self::UPDATE_GAME_MODE)
        .union(Self::UPDATE_LISTED)
        .union(Self::UPDATE_LATENCY)
        .union(Self::UPDATE_DISPLAY_NAME)
        .union(Self::UPDATE_HAT)
        .union(Self::UPDATE_LIST_ORDER);
}

#[derive(Debug, Clone, Default)]
pub struct PlayerInfoEntry {
    pub uuid: Uuid,
    pub name: String,
    pub properties: Vec<Property>,
    pub game_mode: i32,
    pub listed: bool,
    pub latency: i32,
    pub display_name: Option<Nbt>,
    pub list_order: i32,
    pub show_hat: bool,
}

#[derive(Debug, Clone)]
pub struct PlayerInfoUpdate {
    pub actions: PlayerInfoActions,
    pub entries: Vec<PlayerInfoEntry>,
}

impl PlayerInfoUpdate {
    pub fn single(actions: PlayerInfoActions, entry: PlayerInfoEntry) -> Self {
        Self {
            actions,
            entries: vec![entry],
        }
    }
}

impl Encode for PlayerInfoUpdate {
    fn encode(&self, buf: &mut Vec<u8>) {
        let actions = self.actions;
        actions.bits().encode(buf);
        VarI32(self.entries.len() as i32).encode(buf);
        for entry in &self.entries {
            entry.uuid.encode(buf);
            if actions.contains(PlayerInfoActions::ADD_PLAYER) {
                entry.name.encode(buf);
                entry.properties.encode(buf);
            }
            if actions.contains(PlayerInfoActions::INITIALIZE_CHAT) {
                false.encode(buf);
            }
            if actions.contains(PlayerInfoActions::UPDATE_GAME_MODE) {
                VarI32(entry.game_mode).encode(buf);
            }
            if actions.contains(PlayerInfoActions::UPDATE_LISTED) {
                entry.listed.encode(buf);
            }
            if actions.contains(PlayerInfoActions::UPDATE_LATENCY) {
                VarI32(entry.latency).encode(buf);
            }
            if actions.contains(PlayerInfoActions::UPDATE_DISPLAY_NAME) {
                entry.display_name.encode(buf);
            }
            if actions.contains(PlayerInfoActions::UPDATE_LIST_ORDER) {
                VarI32(entry.list_order).encode(buf);
            }
            if actions.contains(PlayerInfoActions::UPDATE_HAT) {
                entry.show_hat.encode(buf);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use ussr_nbt::owned::Tag;

    use super::*;
    use crate::clientbound::ManualPlayPacket;

    fn uuid() -> Uuid {
        Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef)
    }

    fn uuid_bytes() -> Vec<u8> {
        uuid().as_bytes().to_vec()
    }

    fn text(text: &str) -> Nbt {
        Nbt {
            name: "".into(),
            compound: vec![("text".into(), Tag::String(text.into()))].into(),
        }
    }

    fn text_bytes(text: &str) -> Vec<u8> {
        let mut bytes = vec![0x0A, 0x08, 0x00, 0x04, b't', b'e', b'x', b't', 0x00];
        bytes.push(text.len() as u8);
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(0x00);
        bytes
    }

    fn entry() -> PlayerInfoEntry {
        PlayerInfoEntry {
            uuid: uuid(),
            name: "Leo".into(),
            properties: vec![Property {
                name: "textures".into(),
                value: "abc".into(),
                signature: Some("sig".into()),
            }],
            game_mode: 1,
            listed: true,
            latency: 42,
            display_name: Some(text("Leo")),
            list_order: -1,
            show_hat: false,
        }
    }

    #[test]
    fn initializing_matches_paper_layout() {
        let mut buf = Vec::new();
        PlayerInfoUpdate::single(PlayerInfoActions::INITIALIZING, entry()).encode(&mut buf);

        let mut expected = vec![0xFF, 0x01];
        expected.extend(uuid_bytes());
        expected.extend([0x03, b'L', b'e', b'o']);
        expected.extend([0x01, 0x08]);
        expected.extend(b"textures");
        expected.extend([0x03, b'a', b'b', b'c', 0x01, 0x03, b's', b'i', b'g']);
        expected.push(0x00);
        expected.push(0x01);
        expected.push(0x01);
        expected.push(42);
        expected.push(0x01);
        expected.extend(text_bytes("Leo"));
        expected.extend([0xFF, 0xFF, 0xFF, 0xFF, 0x0F]);
        expected.push(0x00);
        assert_eq!(buf, expected);
    }

    #[test]
    fn each_action_writes_only_its_field() {
        let cases: [(PlayerInfoActions, Vec<u8>); 8] = [
            (PlayerInfoActions::ADD_PLAYER, {
                let mut b = vec![0x03, b'L', b'e', b'o', 0x01, 0x08];
                b.extend(b"textures");
                b.extend([0x03, b'a', b'b', b'c', 0x01, 0x03, b's', b'i', b'g']);
                b
            }),
            (PlayerInfoActions::INITIALIZE_CHAT, vec![0x00]),
            (PlayerInfoActions::UPDATE_GAME_MODE, vec![0x01]),
            (PlayerInfoActions::UPDATE_LISTED, vec![0x01]),
            (PlayerInfoActions::UPDATE_LATENCY, vec![42]),
            (PlayerInfoActions::UPDATE_DISPLAY_NAME, {
                let mut b = vec![0x01];
                b.extend(text_bytes("Leo"));
                b
            }),
            (
                PlayerInfoActions::UPDATE_LIST_ORDER,
                vec![0xFF, 0xFF, 0xFF, 0xFF, 0x0F],
            ),
            (PlayerInfoActions::UPDATE_HAT, vec![0x00]),
        ];
        for (action, field) in cases {
            let mut buf = Vec::new();
            PlayerInfoUpdate::single(action, entry()).encode(&mut buf);
            let mut expected = vec![action.bits(), 0x01];
            expected.extend(uuid_bytes());
            expected.extend(field);
            assert_eq!(buf, expected, "{action:?}");
        }
    }

    #[test]
    fn combined_actions_follow_ordinal_order() {
        let mut buf = Vec::new();
        PlayerInfoUpdate::single(
            PlayerInfoActions::UPDATE_HAT
                | PlayerInfoActions::UPDATE_DISPLAY_NAME
                | PlayerInfoActions::UPDATE_LISTED,
            PlayerInfoEntry {
                display_name: None,
                listed: false,
                show_hat: true,
                ..entry()
            },
        )
        .encode(&mut buf);
        let mut expected = vec![0xA8, 0x01];
        expected.extend(uuid_bytes());
        expected.extend([0x00, 0x00, 0x01]);
        assert_eq!(buf, expected);
    }

    #[test]
    fn several_entries_repeat_the_action_fields() {
        let mut buf = Vec::new();
        PlayerInfoUpdate {
            actions: PlayerInfoActions::UPDATE_LATENCY,
            entries: vec![
                PlayerInfoEntry {
                    latency: 1,
                    ..entry()
                },
                PlayerInfoEntry {
                    latency: 300,
                    ..entry()
                },
            ],
        }
        .encode(&mut buf);
        let mut expected = vec![0x10, 0x02];
        expected.extend(uuid_bytes());
        expected.push(0x01);
        expected.extend(uuid_bytes());
        expected.extend([0xAC, 0x02]);
        assert_eq!(buf, expected);
    }

    #[test]
    fn manual_packet_id() {
        let mut buf = Vec::new();
        ManualPlayPacket::PlayerInfoUpdate(PlayerInfoUpdate {
            actions: PlayerInfoActions::empty(),
            entries: Vec::new(),
        })
        .encode(&mut buf);
        assert_eq!(buf, [0x46, 0x00, 0x00]);
    }
}
