use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct UpdateEntityPositionAndRotation {
    #[codec(varint32)]
    pub entity_id: i32,
    pub delta_x: i16,
    pub delta_y: i16,
    pub delta_z: i16,
    pub yaw: u8,
    pub pitch: u8,
    pub on_ground: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::PlayPacket;

    #[test]
    fn matches_paper_move_entity_pos_rot() {
        let mut bytes = Vec::new();
        PlayPacket::UpdateEntityPositionAndRotation(UpdateEntityPositionAndRotation {
            entity_id: 300,
            delta_x: -67,
            delta_y: 0,
            delta_z: 4096,
            yaw: 95,
            pitch: 0,
            on_ground: true,
        })
        .encode(&mut bytes);
        assert_eq!(
            bytes,
            [
                0x36, 0xAC, 0x02, 0xFF, 0xBD, 0x00, 0x00, 0x10, 0x00, 95, 0, 1
            ]
        );
    }
}
