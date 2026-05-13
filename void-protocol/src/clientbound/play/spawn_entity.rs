use crate::types::LpVec3;
use uuid::Uuid;
use voidmc_codec::{Decode, Encode, LpVec3};

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
