use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct SetCompression {
    #[codec(varint32)]
    pub threshold: i32,
}

#[cfg(test)]
mod tests {
    use voidmc_codec::{Decode, Encode};

    use super::*;
    use crate::clientbound::LoginPacket;

    fn encoded(threshold: i32) -> Vec<u8> {
        let mut bytes = Vec::new();
        LoginPacket::SetCompression(SetCompression { threshold }).encode(&mut bytes);
        bytes
    }

    #[test]
    fn set_compression_matches_vanilla_wire_format() {
        assert_eq!(encoded(256), [0x03, 0x80, 0x02]);
        assert_eq!(encoded(0), [0x03, 0x00]);
        assert_eq!(encoded(-1), [0x03, 0xFF, 0xFF, 0xFF, 0xFF, 0x0F]);
    }

    #[test]
    fn set_compression_round_trips() {
        let bytes = encoded(1024);
        let LoginPacket::SetCompression(packet) = LoginPacket::decode(&mut bytes.as_slice()).unwrap()
        else {
            panic!("expected set compression");
        };
        assert_eq!(packet, SetCompression { threshold: 1024 });
    }
}
