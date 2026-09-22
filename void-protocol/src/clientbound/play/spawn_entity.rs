use crate::types::LpVec3;
use uuid::Uuid;
use voidmc_codec::{Decode, Encode};

/// Clientbound `add_entity` packet (1.21.7+).
///
/// Field order matches `net.minecraft.network.protocol.game.ClientboundAddEntityPacket`:
/// `id, uuid, type, x, y, z, movement (LpVec3), xRot, yRot, yHeadRot, data`.
#[derive(Debug, Clone, Encode, Decode)]
pub struct SpawnEntity {
    #[codec(varint32)]
    pub entity_id: i32,
    pub entity_uuid: Uuid,
    #[codec(varint32)]
    pub entity_type: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub velocity: LpVec3,
    pub pitch: u8,
    pub yaw: u8,
    pub head_yaw: u8,
    #[codec(varint32)]
    pub data: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::PlayPacket;

    #[test]
    fn matches_paper_add_entity_with_a_zero_lp_vec3_and_rotation_bytes() {
        let uuid = Uuid::from_u128(0x0123_4567_89ab_cdef_0f1e_2d3c_4b5a_6978);
        let mut bytes = Vec::new();
        PlayPacket::SpawnEntity(SpawnEntity {
            entity_id: 300,
            entity_uuid: uuid,
            entity_type: 85,
            x: -4.5,
            y: 110.0,
            z: -2.0,
            velocity: LpVec3::ZERO,
            pitch: 0,
            yaw: 192,
            head_yaw: 192,
            data: 0,
        })
        .encode(&mut bytes);
        let mut expected = vec![0x01, 0xAC, 0x02];
        expected.extend_from_slice(uuid.as_bytes());
        expected.push(85);
        for value in [-4.5f64, 110.0, -2.0] {
            expected.extend_from_slice(&value.to_be_bytes());
        }
        expected.extend_from_slice(&[0, 0, 192, 192, 0]);
        assert_eq!(bytes, expected);
    }
}
