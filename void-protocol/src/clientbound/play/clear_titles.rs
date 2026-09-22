use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct ClearTitles {
    pub reset_times: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::PlayPacket;

    #[test]
    fn matches_paper_layout() {
        for (reset_times, byte) in [(false, 0x00), (true, 0x01)] {
            let mut buf = Vec::new();
            ClearTitles { reset_times }.encode(&mut buf);
            assert_eq!(buf, [byte]);

            let mut slice = buf.as_slice();
            let decoded = ClearTitles::decode(&mut slice).unwrap();
            assert!(slice.is_empty());
            assert_eq!(decoded.reset_times, reset_times);
        }
    }

    #[test]
    fn tagged_packet_id() {
        let mut buf = Vec::new();
        PlayPacket::ClearTitles(ClearTitles { reset_times: true }).encode(&mut buf);
        assert_eq!(buf, [0x0E, 0x01]);
    }
}
