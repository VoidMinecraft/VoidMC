use voidmc_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct CommandSuggestionsRequest {
    #[codec(varint32)]
    pub transaction_id: i32,
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_varint_id_then_utf8_command() {
        let mut bytes = vec![0x81, 0x01, 8];
        bytes.extend(b"/tp ~ ~ ");
        let packet = CommandSuggestionsRequest::decode(&mut bytes.as_slice()).unwrap();
        assert_eq!(packet.transaction_id, 129);
        assert_eq!(packet.text, "/tp ~ ~ ");
    }
}
