use ussr_nbt::owned::Nbt;
use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct SetTitleText {
    pub text: Nbt,
}

#[cfg(test)]
pub(crate) fn text_nbt(text: &str) -> Nbt {
    use ussr_nbt::owned::Tag;

    Nbt {
        name: "".into(),
        compound: vec![("text".into(), Tag::String(text.into()))].into(),
    }
}

#[cfg(test)]
pub(crate) fn text_nbt_bytes(text: &str) -> Vec<u8> {
    [
        &[0x0A, 0x08, 0x00, 0x04][..],
        b"text",
        &[0x00, text.len() as u8],
        text.as_bytes(),
        &[0x00],
    ]
    .concat()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::PlayPacket;

    #[test]
    fn matches_paper_layout() {
        let mut buf = Vec::new();
        SetTitleText {
            text: text_nbt("Round 2"),
        }
        .encode(&mut buf);
        assert_eq!(buf, text_nbt_bytes("Round 2"));

        let mut slice = buf.as_slice();
        let decoded = SetTitleText::decode(&mut slice).unwrap();
        assert!(slice.is_empty());
        assert_eq!(decoded.text.compound.tags.len(), 1);
    }

    #[test]
    fn tagged_packet_id() {
        let mut buf = Vec::new();
        PlayPacket::SetTitleText(SetTitleText {
            text: text_nbt("a"),
        })
        .encode(&mut buf);
        assert_eq!(buf[0], 0x72);
        assert_eq!(&buf[1..], text_nbt_bytes("a"));
    }
}
