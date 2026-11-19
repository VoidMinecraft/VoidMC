use voidmc_codec::{Decode, Encode};

use crate::types::BlockPosition;

/// `minecraft:block_event` (wiki: Block Action): makes the client run a
/// block's event handler, e.g. a note block's note particle, a chest lid or a
/// piston. The meaning of `action_id` and `action_parameter` depends on
/// `block_type`, the `minecraft:block` registry id of the block at `position`.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct BlockEvent {
    pub position: BlockPosition,
    pub action_id: u8,
    pub action_parameter: u8,
    #[codec(varint32)]
    pub block_type: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::PlayPacket;

    #[test]
    fn layout_is_position_two_bytes_then_block_varint() {
        let position = BlockPosition { x: 1, y: 64, z: -2 };
        let mut bytes = Vec::new();
        PlayPacket::BlockEvent(BlockEvent {
            position,
            action_id: 0,
            action_parameter: 3,
            block_type: 300,
        })
        .encode(&mut bytes);

        let mut expected = vec![0x07];
        position.encode(&mut expected);
        expected.extend([0, 3, 0xAC, 0x02]);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn roundtrips() {
        let packet = BlockEvent {
            position: BlockPosition {
                x: -30_000,
                y: -64,
                z: 12,
            },
            action_id: 1,
            action_parameter: 255,
            block_type: 109,
        };
        let mut bytes = Vec::new();
        packet.encode(&mut bytes);
        let mut slice = bytes.as_slice();
        assert_eq!(BlockEvent::decode(&mut slice).unwrap(), packet);
        assert!(slice.is_empty());
    }
}
