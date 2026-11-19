use voidmc_codec::{Decode, Encode};

/// Serverbound Chat Acknowledgement (0x06): the number of signed player chat
/// messages the client has seen since its last acknowledgement. A vanilla
/// client only sends it after receiving Player Chat packets.
#[derive(Debug, Encode, Decode)]
pub struct ChatAck {
    #[codec(varint32)]
    pub offset: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_varint_offset() {
        let packet = ChatAck::decode(&mut [0xAC, 0x02].as_slice()).unwrap();
        assert_eq!(packet.offset, 300);
    }
}
