use ussr_nbt::owned::Nbt;
use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct SetSubtitleText {
    pub text: Nbt,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::PlayPacket;
    use crate::clientbound::play::set_title_text::{text_nbt, text_nbt_bytes};

    #[test]
    fn matches_paper_layout() {
        let mut buf = Vec::new();
        SetSubtitleText {
            text: text_nbt("Get ready"),
        }
        .encode(&mut buf);
        assert_eq!(buf, text_nbt_bytes("Get ready"));

        let mut slice = buf.as_slice();
        let decoded = SetSubtitleText::decode(&mut slice).unwrap();
        assert!(slice.is_empty());
        assert_eq!(decoded.text.compound.tags.len(), 1);
    }

    #[test]
    fn tagged_packet_id() {
        let mut buf = Vec::new();
        PlayPacket::SetSubtitleText(SetSubtitleText {
            text: text_nbt("a"),
        })
        .encode(&mut buf);
        assert_eq!(buf[0], 0x70);
        assert_eq!(&buf[1..], text_nbt_bytes("a"));
    }
}
