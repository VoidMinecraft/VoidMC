use voidmc_codec::{Decode, Encode};

use crate::slot::Slot;

/// Replaces the entire contents of a container window (`CLIENTBOUND_CONTAINER_SET_CONTENT`).
///
/// `container_id` 0 is the player's own inventory. `items` is the full slot list
/// in window order; `carried` is the item currently held on the cursor.
#[derive(Debug, Clone, Encode, Decode)]
pub struct SetContainerContent {
    #[codec(varint32)]
    pub container_id: i32,
    #[codec(varint32)]
    pub state_id: i32,
    pub items: Vec<Slot>,
    pub carried: Slot,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::PlayPacket;

    #[test]
    fn set_container_content_roundtrips() {
        let packet = SetContainerContent {
            container_id: 0,
            state_id: 1,
            items: vec![Slot::simple(1, 64), Slot::EMPTY, Slot::simple(937, 1)],
            carried: Slot::EMPTY,
        };
        let mut buf = Vec::new();
        packet.encode(&mut buf);
        let mut slice = buf.as_slice();
        let decoded = SetContainerContent::decode(&mut slice).unwrap();
        assert_eq!(slice.len(), 0);
        assert_eq!(decoded.items.len(), 3);
        assert_eq!(decoded.items[0], Slot::simple(1, 64));
        assert_eq!(decoded.items[2].item_id, 937);
    }

    #[test]
    fn tagged_packet_id_is_0x12() {
        let packet = PlayPacket::SetContainerContent(SetContainerContent {
            container_id: 0,
            state_id: 0,
            items: vec![],
            carried: Slot::EMPTY,
        });
        let mut buf = Vec::new();
        packet.encode(&mut buf);
        // The tagged-enum packet id byte for SetContainerContent is 0x12.
        assert_eq!(buf[0], 0x12);
    }
}
