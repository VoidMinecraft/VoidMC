use voidmc_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct ConfirmTeleportation {
    #[codec(varint32)]
    pub teleport_id: i32,
}
