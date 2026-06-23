use crate::types::LpVec3;
use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct SetEntityMotion {
    #[codec(varint32)]
    pub entity_id: i32,
    pub velocity: LpVec3,
}
