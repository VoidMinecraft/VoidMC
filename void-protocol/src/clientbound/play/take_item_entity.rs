use voidmc_codec::{Decode, Encode};

/// Plays the pickup animation: the item flies into the collector. The client
/// also shrinks its copy of the item's stack by `amount` and drops the entity
/// once it is empty; the server still sends `RemoveEntities`.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct TakeItemEntity {
    #[codec(varint32)]
    pub collected_entity_id: i32,
    #[codec(varint32)]
    pub collector_entity_id: i32,
    #[codec(varint32)]
    pub amount: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::PlayPacket;

    #[test]
    fn encodes_three_varints_behind_its_packet_id() {
        let packet = TakeItemEntity {
            collected_entity_id: 300,
            collector_entity_id: 7,
            amount: 64,
        };
        let mut buf = Vec::new();
        PlayPacket::TakeItemEntity(packet.clone()).encode(&mut buf);
        assert_eq!(buf, [0x7C, 0xAC, 0x02, 0x07, 0x40]);

        let mut slice = &buf[1..];
        assert_eq!(TakeItemEntity::decode(&mut slice).unwrap(), packet);
        assert!(slice.is_empty());
    }
}
