use voidmc_codec::{Decode, Encode};

use crate::types::{BlockFace, BlockPosition, Hand};

#[derive(Debug, Encode, Decode)]
pub struct UseItemOn {
    pub hand: Hand,
    pub location: BlockPosition,
    pub face: BlockFace,
    pub cursor_x: f32,
    pub cursor_y: f32,
    pub cursor_z: f32,
    pub inside_block: bool,
    pub world_border_hit: bool,
    #[codec(varint32)]
    pub sequence: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serverbound::PlayPacket;

    #[test]
    fn decodes_official_26_1_2_use_item_on_frame() {
        let position = BlockPosition { x: 3, y: 64, z: 5 };
        let mut bytes = vec![0x42, Hand::MainHand as u8];
        position.encode(&mut bytes);
        bytes.push(BlockFace::Top as u8);
        0.5_f32.encode(&mut bytes);
        1.0_f32.encode(&mut bytes);
        0.25_f32.encode(&mut bytes);
        false.encode(&mut bytes);
        false.encode(&mut bytes);
        bytes.push(9);

        let mut input = bytes.as_slice();
        let packet = PlayPacket::decode(&mut input).unwrap();
        let PlayPacket::UseItemOn(use_item_on) = packet else {
            panic!("expected use item on");
        };

        assert_eq!(use_item_on.hand, Hand::MainHand);
        assert_eq!(use_item_on.location, position);
        assert_eq!(use_item_on.face, BlockFace::Top);
        assert_eq!(use_item_on.cursor_x, 0.5);
        assert_eq!(use_item_on.cursor_y, 1.0);
        assert_eq!(use_item_on.cursor_z, 0.25);
        assert!(!use_item_on.inside_block);
        assert!(!use_item_on.world_border_hit);
        assert_eq!(use_item_on.sequence, 9);
        assert!(input.is_empty());
    }
}
