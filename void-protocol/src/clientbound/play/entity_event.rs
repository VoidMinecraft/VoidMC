use voidmc_codec::{Decode, Encode};

/// `minecraft:entity_event` (wiki: Entity Event, formerly Entity Status):
/// makes the client run one of an entity's status handlers. The meaning of
/// `event_id` depends on the entity's type; on the client's own player,
/// 24 to 28 set its permission level to `event_id - 24`
/// ([`EntityEvent::permission_level`]).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct EntityEvent {
    /// The entity's id, as a plain int rather than a VarInt.
    pub entity_id: i32,
    pub event_id: u8,
}

impl EntityEvent {
    /// The status for permission level 0.
    pub const PERMISSION_LEVEL_0: u8 = 24;
    /// The status for permission level 4, the highest.
    pub const PERMISSION_LEVEL_4: u8 = 28;

    /// Tells the client whose player is `entity_id` that its permission
    /// level is `level` (0 to 4, clamped).
    pub fn permission_level(entity_id: i32, level: u8) -> Self {
        Self {
            entity_id,
            event_id: Self::PERMISSION_LEVEL_0 + level.min(4),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::PlayPacket;

    #[test]
    fn layout_is_an_int_then_a_byte() {
        let mut bytes = Vec::new();
        PlayPacket::EntityEvent(EntityEvent::permission_level(300, 4)).encode(&mut bytes);
        assert_eq!(bytes, [0x22, 0x00, 0x00, 0x01, 0x2C, 28]);
    }

    #[test]
    fn permission_levels_map_to_statuses_24_to_28() {
        let statuses: Vec<u8> = (0..=5)
            .map(|level| EntityEvent::permission_level(1, level).event_id)
            .collect();
        assert_eq!(statuses, [24, 25, 26, 27, 28, 28]);
    }

    #[test]
    fn roundtrips() {
        let packet = EntityEvent {
            entity_id: -7,
            event_id: 24,
        };
        let mut bytes = Vec::new();
        packet.encode(&mut bytes);
        let mut slice = bytes.as_slice();
        assert_eq!(EntityEvent::decode(&mut slice).unwrap(), packet);
        assert!(slice.is_empty());
    }
}
