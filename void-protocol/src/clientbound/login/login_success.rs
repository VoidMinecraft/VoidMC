use uuid::Uuid;
use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct Property {
    pub name: String,
    pub value: String,
    pub signature: Option<String>,
}

#[derive(Debug, Encode, Decode)]
pub struct LoginSuccess {
    pub uuid: Uuid,
    pub username: String,
    pub properties: Vec<Property>,
}
