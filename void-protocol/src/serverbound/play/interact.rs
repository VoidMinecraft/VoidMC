use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct Interact {
    #[codec(varint32)]
    pub entity_id: i32,
    #[codec(varint32)]
    pub interact_type: i32,
    #[codec(remaining)]
    pub _data: Vec<u8>,
}
