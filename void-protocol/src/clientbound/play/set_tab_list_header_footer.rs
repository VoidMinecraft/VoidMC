use ussr_nbt::owned::Nbt;
use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct SetTabListHeaderFooter {
    pub header: Nbt,
    pub footer: Nbt,
}

#[cfg(test)]
mod tests {
    use ussr_nbt::owned::Tag;

    use super::*;
    use crate::clientbound::PlayPacket;

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

    #[test]
    fn matches_paper_layout() {
        let mut buf = Vec::new();
        PlayPacket::SetTabListHeaderFooter(SetTabListHeaderFooter {
            header: text("Alpine Rush"),
            footer: text(""),
        })
        .encode(&mut buf);

        let mut expected = vec![0x7A];
        expected.extend(text_bytes("Alpine Rush"));
        expected.extend(text_bytes(""));
        assert_eq!(buf, expected);

        let mut slice = buf[1..].as_ref();
        let decoded = SetTabListHeaderFooter::decode(&mut slice).unwrap();
        assert!(slice.is_empty());
        assert_eq!(decoded.header, text("Alpine Rush"));
        assert_eq!(decoded.footer, text(""));
    }
}
