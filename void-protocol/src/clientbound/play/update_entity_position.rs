use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct UpdateEntityPosition {
    #[codec(varint32)]
    pub entity_id: i32,
    pub delta_x: i16,
    pub delta_y: i16,
    pub delta_z: i16,
    pub on_ground: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::PlayPacket;

    #[test]
    fn matches_paper_move_entity_pos() {
        let mut bytes = Vec::new();
        PlayPacket::UpdateEntityPosition(UpdateEntityPosition {
            entity_id: 300,
            delta_x: 1024,
            delta_y: -8192,
            delta_z: 0,
            on_ground: false,
        })
        .encode(&mut bytes);
        assert_eq!(
            bytes,
            [0x35, 0xAC, 0x02, 0x04, 0x00, 0xE0, 0x00, 0x00, 0x00, 0]
        );
    }
}
