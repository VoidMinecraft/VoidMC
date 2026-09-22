use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct EntityPositionSync {
    #[codec(varint32)]
    pub entity_id: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub vx: f64,
    pub vy: f64,
    pub vz: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::PlayPacket;

    #[test]
    fn matches_paper_id_position_move_rotation_and_on_ground() {
        let mut bytes = Vec::new();
        PlayPacket::EntityPositionSync(EntityPositionSync {
            entity_id: 300,
            x: 1.5,
            y: -2.0,
            z: 3.25,
            vx: 0.5,
            vy: 0.0,
            vz: -0.25,
            yaw: 90.0,
            pitch: -45.0,
            on_ground: true,
        })
        .encode(&mut bytes);
        let mut expected = vec![0x23, 0xAC, 0x02];
        for value in [1.5f64, -2.0, 3.25, 0.5, 0.0, -0.25] {
            expected.extend_from_slice(&value.to_be_bytes());
        }
        expected.extend_from_slice(&90.0f32.to_be_bytes());
        expected.extend_from_slice(&(-45.0f32).to_be_bytes());
        expected.push(1);
        assert_eq!(bytes, expected);
        assert_eq!(bytes.len(), 3 + 6 * 8 + 2 * 4 + 1);
    }
}
