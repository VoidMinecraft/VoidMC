use ussr_nbt::owned::Nbt;
use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct OpenScreen {
    #[codec(varint32)]
    pub container_id: i32,
    #[codec(varint32)]
    pub menu_type: i32,
    pub title: Nbt,
}

#[cfg(test)]
mod tests {
    use ussr_nbt::owned::Tag;

    use super::*;
    use crate::clientbound::PlayPacket;

    fn title(text: &str) -> Nbt {
        Nbt {
            name: "".into(),
            compound: vec![("text".into(), Tag::String(text.into()))].into(),
        }
    }

    #[test]
    fn matches_paper_layout() {
        let packet = OpenScreen {
            container_id: 3,
            menu_type: 16,
            title: title("Hi"),
        };
        let mut buf = Vec::new();
        packet.encode(&mut buf);
        let mut expected = vec![0x03, 0x10];
        title("Hi").encode(&mut expected);
        assert_eq!(buf, expected);

        let mut slice = buf.as_slice();
        let decoded = OpenScreen::decode(&mut slice).unwrap();
        assert!(slice.is_empty());
        assert_eq!(decoded.container_id, 3);
        assert_eq!(decoded.menu_type, 16);
    }

    #[test]
    fn container_id_is_a_varint() {
        let packet = OpenScreen {
            container_id: 200,
            menu_type: 0,
            title: title(""),
        };
        let mut buf = Vec::new();
        packet.encode(&mut buf);
        assert_eq!(&buf[..3], &[0xC8, 0x01, 0x00]);
    }

    #[test]
    fn tagged_packet_id() {
        let packet = PlayPacket::OpenScreen(OpenScreen {
            container_id: 1,
            menu_type: 0,
            title: title(""),
        });
        let mut buf = Vec::new();
        packet.encode(&mut buf);
        assert_eq!(buf[0], 0x3B);
    }
}
