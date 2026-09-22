use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct SetTitlesAnimation {
    pub fade_in: i32,
    pub stay: i32,
    pub fade_out: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::PlayPacket;

    #[test]
    fn matches_paper_layout() {
        let mut buf = Vec::new();
        SetTitlesAnimation {
            fade_in: 10,
            stay: 70,
            fade_out: -1,
        }
        .encode(&mut buf);
        assert_eq!(
            buf,
            [
                0x00, 0x00, 0x00, 0x0A, 0x00, 0x00, 0x00, 0x46, 0xFF, 0xFF, 0xFF, 0xFF
            ]
        );

        let mut slice = buf.as_slice();
        let decoded = SetTitlesAnimation::decode(&mut slice).unwrap();
        assert!(slice.is_empty());
        assert_eq!(
            decoded,
            SetTitlesAnimation {
                fade_in: 10,
                stay: 70,
                fade_out: -1
            }
        );
    }

    #[test]
    fn tagged_packet_id() {
        let mut buf = Vec::new();
        PlayPacket::SetTitlesAnimation(SetTitlesAnimation {
            fade_in: 0,
            stay: 20,
            fade_out: 5,
        })
        .encode(&mut buf);
        assert_eq!(
            buf,
            [
                0x73, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x14, 0x00, 0x00, 0x00, 0x05
            ]
        );
    }
}
