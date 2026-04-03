use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct CommandSuggestionsRequest {
    #[codec(varint32)]
    pub transaction_id: i32,
    pub text: String,
}
