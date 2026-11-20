use voidmc_codec::{Decode, Encode};

use crate::types::{Hand, LpVec3};

/// A right-click on an entity (`minecraft:interact`). Attacks are a separate
/// packet, [`Attack`](super::Attack).
#[derive(Debug, Encode, Decode)]
pub struct Interact {
    #[codec(varint32)]
    pub entity_id: i32,
    pub hand: Hand,
    /// Where the click hit, relative to the entity's position.
    pub location: LpVec3,
    /// Whether the sneak key was pressed during the click.
    pub using_secondary_action: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serverbound::PlayPacket;
    use voidmc_codec::DecodeLimits;

    fn decode(bytes: &[u8]) -> Interact {
        match DecodeLimits::default().decode_exact::<PlayPacket>(bytes) {
            Ok(PlayPacket::Interact(interact)) => interact,
            other => panic!("expected an interact packet, got {other:?}"),
        }
    }

    // Frames as Azalea 0.16.0 writes `ServerboundInteract`: entity (VarInt),
    // hand (VarInt), location (`LpVec3`), sneaking (bool).
    #[test]
    fn decodes_a_main_hand_click_as_azalea_writes_it() {
        let interact = decode(&[0x1A, 5, 0, 0xF9, 0xFF, 0x7F, 0xFF, 0xFF, 0xF9, 0]);
        assert_eq!(interact.entity_id, 5);
        assert_eq!(interact.hand, Hand::MainHand);
        assert_eq!(
            interact.location,
            LpVec3 {
                x: 0.0,
                y: 1.0,
                z: 0.0
            }
        );
        assert!(!interact.using_secondary_action);
    }

    #[test]
    fn decodes_a_sneaking_off_hand_click_as_azalea_writes_it() {
        // Entity 300 (VarInt AC 02), off hand, (0.3, 1.62, -0.25), sneaking.
        let interact = decode(&[0x1A, 0xAC, 0x02, 1, 0xC2, 0x4C, 0x6F, 0xFF, 0xCF, 0x56, 1]);
        assert_eq!(interact.entity_id, 300);
        assert_eq!(interact.hand, Hand::OffHand);
        assert!((interact.location.x - 0.3).abs() < 1e-3);
        assert!((interact.location.y - 1.62).abs() < 1e-3);
        assert!((interact.location.z + 0.25).abs() < 1e-3);
        assert!(interact.using_secondary_action);
    }

    #[test]
    fn decodes_a_zero_location_as_one_byte() {
        let interact = decode(&[0x1A, 7, 1, 0, 1]);
        assert_eq!(interact.entity_id, 7);
        assert_eq!(interact.hand, Hand::OffHand);
        assert_eq!(interact.location, LpVec3::ZERO);
        assert!(interact.using_secondary_action);
    }

    #[test]
    fn encodes_the_26_1_2_layout() {
        let mut bytes = Vec::new();
        Interact {
            entity_id: 5,
            hand: Hand::MainHand,
            location: LpVec3 {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
            using_secondary_action: true,
        }
        .encode(&mut bytes);
        assert_eq!(bytes, [5, 0, 0xF9, 0xFF, 0x7F, 0xFF, 0xFF, 0xF9, 1]);
    }

    #[test]
    fn rejects_the_pre_26_1_attack_form() {
        // Old layout: entity, type 1 (attack), sneaking. No location follows.
        assert!(
            DecodeLimits::default()
                .decode_exact::<PlayPacket>(&[0x1A, 5, 1, 0])
                .is_err()
        );
    }
}
