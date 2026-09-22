use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct UpdateEntityRotation {
    #[codec(varint32)]
    pub entity_id: i32,
    pub yaw: u8,
    pub pitch: u8,
    pub on_ground: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::PlayPacket;

    #[test]
    fn matches_paper_move_entity_rot() {
        let mut bytes = Vec::new();
        PlayPacket::UpdateEntityRotation(UpdateEntityRotation {
            entity_id: 300,
            yaw: 192,
            pitch: 224,
            on_ground: true,
        })
        .encode(&mut bytes);
        assert_eq!(bytes, [0x38, 0xAC, 0x02, 192, 224, 1]);
    }
}
