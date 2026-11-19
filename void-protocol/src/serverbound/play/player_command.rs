use voidmc_codec::{Decode, Encode};

use crate::types::PlayerCommandAction;

#[derive(Debug, Encode, Decode)]
pub struct PlayerCommand {
    #[codec(varint32)]
    pub entity_id: i32,
    pub action_id: PlayerCommandAction,
    #[codec(varint32)]
    pub jump_boost: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serverbound::PlayPacket;

    fn decode_action(action: u8) -> PlayerCommandAction {
        let bytes = [0x2A, 7, action, 0];
        let mut input = bytes.as_slice();
        let PlayPacket::PlayerCommand(command) = PlayPacket::decode(&mut input).unwrap() else {
            panic!("expected a player command");
        };
        assert!(input.is_empty());
        assert_eq!(command.entity_id, 7);
        command.action_id
    }

    #[test]
    fn decodes_the_26_1_2_action_numbering() {
        let expected = [
            PlayerCommandAction::LeaveBed,
            PlayerCommandAction::StartSprinting,
            PlayerCommandAction::StopSprinting,
            PlayerCommandAction::StartJumpWithHorse,
            PlayerCommandAction::StopJumpWithHorse,
            PlayerCommandAction::OpenVehicleInventory,
            PlayerCommandAction::StartFlyingWithElytra,
        ];
        for (id, action) in expected.into_iter().enumerate() {
            assert_eq!(decode_action(id as u8), action, "action id {id}");
        }
    }

    #[test]
    fn rejects_an_action_id_past_the_last() {
        let bytes = [0x2A, 7, 7, 0];
        assert!(PlayPacket::decode(&mut bytes.as_slice()).is_err());
    }
}
