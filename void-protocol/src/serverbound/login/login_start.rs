use uuid::Uuid;
use voidmc_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct LoginStart {
    pub name: String,
    pub uuid: Uuid,
}
