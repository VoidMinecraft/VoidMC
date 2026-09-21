use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct CloseContainer {
    #[codec(varint32)]
    pub container_id: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::PlayPacket;

    #[test]
    fn container_id_is_a_varint() {
        let mut buf = Vec::new();
        CloseContainer { container_id: 130 }.encode(&mut buf);
        assert_eq!(buf, vec![0x82, 0x01]);
        let mut slice = buf.as_slice();
        assert_eq!(
            CloseContainer::decode(&mut slice).unwrap().container_id,
            130
        );
        assert!(slice.is_empty());
    }

    #[test]
    fn tagged_packet_id() {
        let mut buf = Vec::new();
        PlayPacket::CloseContainer(CloseContainer { container_id: 1 }).encode(&mut buf);
        assert_eq!(buf, vec![0x11, 0x01]);
    }
}
