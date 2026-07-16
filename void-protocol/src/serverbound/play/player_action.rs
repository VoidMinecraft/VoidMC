use voidmc_codec::{Decode, Encode};

use crate::types::{BlockFace, BlockPosition, PlayerActionStatus};

#[derive(Debug, Encode, Decode)]
pub struct PlayerAction {
    pub status: PlayerActionStatus,
    pub position: BlockPosition,
    pub face: BlockFace,
    #[codec(varint32)]
    pub sequence: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serverbound::PlayPacket;

    #[test]
    fn decodes_official_26_1_2_player_action_frame() {
        let position = BlockPosition { x: 3, y: 64, z: 5 };
        let mut bytes = vec![0x29, PlayerActionStatus::StartedDigging as u8];
        position.encode(&mut bytes);
        bytes.push(BlockFace::Top as u8);
        bytes.push(9);

        let mut input = bytes.as_slice();
        let packet = PlayPacket::decode(&mut input).unwrap();
        let PlayPacket::PlayerAction(action) = packet else {
            panic!("expected player action");
        };

        assert_eq!(action.status, PlayerActionStatus::StartedDigging);
        assert_eq!(action.position, position);
        assert_eq!(action.face, BlockFace::Top);
        assert_eq!(action.sequence, 9);
        assert!(input.is_empty());
    }
}
