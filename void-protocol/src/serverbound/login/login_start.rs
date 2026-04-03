use uuid::Uuid;
use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct LoginStart {
    pub name: String,
    pub uuid: Uuid,
}
