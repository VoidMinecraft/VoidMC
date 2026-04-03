use uuid::Uuid;
use void_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct Property {
    pub name: String,
    pub value: String,
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct LoginSuccess {
    pub uuid: Uuid,
    pub username: String,
    pub properties: Vec<Property>,
}
