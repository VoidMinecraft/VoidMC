mod primitives;

pub use primitives::*;
pub use void_codec_macros::{Decode, Encode};

pub trait Encode {
    fn encode(&self, buf: &mut Vec<u8>);
}

pub trait Decode: Sized {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    UnexpectedEof,
    InvalidVarintLength,
    InvalidPacketId,
    InvalidLength,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as void_codec;

    #[test]
    fn test_struct_with_mixed_fields() {
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct TestPacket {
            pub a: u8,
            pub b: i32,
            pub c: bool,
        }

        let packet = TestPacket {
            a: 42,
            b: 12345,
            c: true,
        };

        let mut buf = Vec::new();
        packet.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = TestPacket::decode(&mut slice).unwrap();

        assert_eq!(decoded, packet);
        assert_eq!(slice.len(), 0);
    }

    #[test]
    fn test_struct_with_vari32() {
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct VarPacket {
            pub prefix: u8,
            #[codec(varint32)]
            pub value: i32,
        }

        let packet = VarPacket {
            prefix: 1,
            value: 12345,
        };

        let mut buf = Vec::new();
        packet.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = VarPacket::decode(&mut slice).unwrap();

        assert_eq!(decoded, packet);
        assert_eq!(slice.len(), 0);
    }

    #[test]
    fn test_struct_with_vari32_compression() {
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct SmallVarPacket {
            #[codec(varint32)]
            pub value: i32,
        }

        let packet = SmallVarPacket { value: 100 };

        let mut buf = Vec::new();
        packet.encode(&mut buf);

        let small_vari_size = buf.len();

        let packet2 = SmallVarPacket { value: i32::MAX };

        let mut buf2 = Vec::new();
        packet2.encode(&mut buf2);

        let large_vari_size = buf2.len();

        assert!(small_vari_size < large_vari_size);
        assert_eq!(small_vari_size, 1);
        assert_eq!(large_vari_size, 5);
    }

    #[test]
    fn test_tagged_enum() {
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct Packet1 {
            pub data: u8,
        }

        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct Packet2 {
            pub value: i32,
        }

        #[derive(void_codec_macros::Encode, void_codec_macros::Decode)]
        #[codec(tagged)]
        pub enum StatePacket {
            #[codec(packet_id = 0)]
            First(Packet1),
            #[codec(packet_id = 1)]
            Second(Packet2),
        }

        let packet = StatePacket::First(Packet1 { data: 42 });

        let mut buf = Vec::new();
        packet.encode(&mut buf);

        assert_eq!(buf[0], 0);

        let mut slice = buf.as_slice();
        let decoded = StatePacket::decode(&mut slice).unwrap();

        match decoded {
            StatePacket::First(p) => assert_eq!(p.data, 42),
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_tagged_enum_second_variant() {
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct Packet1 {
            pub data: u8,
        }

        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct Packet2 {
            pub value: i32,
        }

        #[derive(void_codec_macros::Encode, void_codec_macros::Decode)]
        #[codec(tagged)]
        pub enum StatePacket {
            #[codec(packet_id = 0)]
            First(Packet1),
            #[codec(packet_id = 1)]
            Second(Packet2),
        }

        let packet = StatePacket::Second(Packet2 { value: 12345 });

        let mut buf = Vec::new();
        packet.encode(&mut buf);

        assert_eq!(buf[0], 1);

        let mut slice = buf.as_slice();
        let decoded = StatePacket::decode(&mut slice).unwrap();

        match decoded {
            StatePacket::Second(p) => assert_eq!(p.value, 12345),
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_tagged_enum_invalid_id() {
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, Debug, PartialEq)]
        #[codec(tagged)]
        pub enum StatePacket {
            #[codec(packet_id = 0)]
            First(u8),
        }

        let buf = vec![255];
        let mut slice = buf.as_slice();
        let result = StatePacket::decode(&mut slice);

        assert_eq!(result, Err(DecodeError::InvalidPacketId));
    }

    #[test]
    fn test_unit_struct() {
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct UnitPacket;

        let packet = UnitPacket;

        let mut buf = Vec::new();
        packet.encode(&mut buf);

        assert_eq!(buf.len(), 0);

        let mut slice = buf.as_slice();
        let decoded = UnitPacket::decode(&mut slice).unwrap();

        assert_eq!(decoded, packet);
    }

    #[test]
    fn test_empty_decode() {
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct TestPacket {
            pub a: i32,
        }

        let mut slice = &[][..];
        let result = TestPacket::decode(&mut slice);

        assert_eq!(result, Err(DecodeError::UnexpectedEof));
    }

    #[test]
    fn test_multiple_vari32_fields() {
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct MultiVarPacket {
            #[codec(varint32)]
            pub a: i32,
            #[codec(varint32)]
            pub b: i32,
            #[codec(varint32)]
            pub c: i32,
        }

        let packet = MultiVarPacket {
            a: 1,
            b: 128,
            c: -1,
        };

        let mut buf = Vec::new();
        packet.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = MultiVarPacket::decode(&mut slice).unwrap();

        assert_eq!(decoded, packet);
    }

    #[test]
    fn test_mixed_vari32_and_regular() {
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct MixedPacket {
            pub regular: u8,
            #[codec(varint32)]
            pub compact: i32,
            pub regular2: i32,
            #[codec(varint32)]
            pub compact2: i32,
        }

        let packet = MixedPacket {
            regular: 255,
            compact: 100,
            regular2: 0x12345678,
            compact2: 50000,
        };

        let mut buf = Vec::new();
        packet.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = MixedPacket::decode(&mut slice).unwrap();

        assert_eq!(decoded, packet);
    }

    #[test]
    fn test_nested_structs() {
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct Inner {
            pub value: u8,
        }

        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct Outer {
            pub inner: Inner,
            pub extra: i32,
        }

        let packet = Outer {
            inner: Inner { value: 42 },
            extra: 12345,
        };

        let mut buf = Vec::new();
        packet.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = Outer::decode(&mut slice).unwrap();

        assert_eq!(decoded, packet);
    }

    #[test]
    fn test_large_varint() {
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct LargeVarPacket {
            #[codec(varint32)]
            pub value: i32,
        }

        let packet = LargeVarPacket { value: 268435455 };

        let mut buf = Vec::new();
        packet.encode(&mut buf);

        assert_eq!(buf.len(), 4);

        let mut slice = buf.as_slice();
        let decoded = LargeVarPacket::decode(&mut slice).unwrap();

        assert_eq!(decoded, packet);
    }

    #[test]
    fn test_repr_u8_enum() {
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        #[repr(u8)]
        pub enum SimpleEnum {
            First = 0,
            Second = 1,
            Third = 42,
        }

        for variant in [SimpleEnum::First, SimpleEnum::Second, SimpleEnum::Third] {
            let mut buf = Vec::new();
            variant.encode(&mut buf);

            assert_eq!(buf.len(), 1);

            let mut slice = buf.as_slice();
            let decoded = SimpleEnum::decode(&mut slice).unwrap();

            assert_eq!(decoded, variant);
            assert_eq!(slice.len(), 0);
        }
    }

    #[test]
    fn test_repr_i32_enum() {
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        #[repr(i32)]
        pub enum StatusEnum {
            Pending = -1,
            Active = 0,
            Complete = 1,
        }

        for variant in [
            StatusEnum::Pending,
            StatusEnum::Active,
            StatusEnum::Complete,
        ] {
            let mut buf = Vec::new();
            variant.encode(&mut buf);

            assert_eq!(buf.len(), 4);

            let mut slice = buf.as_slice();
            let decoded = StatusEnum::decode(&mut slice).unwrap();

            assert_eq!(decoded, variant);
            assert_eq!(slice.len(), 0);
        }
    }

    #[test]
    fn test_repr_i32_enum_varint32_compression() {
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        #[codec(varint32)]
        #[repr(i32)]
        pub enum CompressedEnum {
            Small = 0,
            Medium = 100,
            Large = 268435455,
        }

        let variant = CompressedEnum::Large;
        let mut buf = Vec::new();
        variant.encode(&mut buf);

        assert_eq!(buf.len(), 4);

        let mut slice = buf.as_slice();
        let decoded = CompressedEnum::decode(&mut slice).unwrap();

        assert_eq!(decoded, variant);
    }

    #[test]
    fn test_repr_u8_enum_invalid_discriminant() {
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        #[repr(u8)]
        pub enum StatusEnum {
            Active = 1,
            Inactive = 2,
        }

        let buf = vec![99u8];
        match StatusEnum::decode(&mut buf.as_slice()) {
            Err(DecodeError::InvalidPacketId) => (),
            other => panic!("Expected InvalidPacketId error, got {:?}", other),
        }
    }

    #[test]
    fn test_repr_i32_enum_varint32() {
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        #[codec(varint32)]
        #[repr(i32)]
        pub enum VarIntEnum {
            Zero = 0,
            One = 1,
            Hundred = 100,
        }

        let mut buf = Vec::new();
        VarIntEnum::Hundred.encode(&mut buf);

        assert!(buf.len() < 4);

        let mut slice = buf.as_slice();
        let decoded = VarIntEnum::decode(&mut slice).unwrap();

        assert_eq!(decoded, VarIntEnum::Hundred);
    }
}
