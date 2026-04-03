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
    InvalidPacketId(Option<u8>),
    InvalidLength,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::UnexpectedEof => write!(f, "Unexpected end of stream"),
            DecodeError::InvalidVarintLength => write!(f, "Invalid variable-length integer"),
            DecodeError::InvalidPacketId(Some(id)) => {
                write!(f, "Unsupported packet id 0x{:02X}", id)
            }
            DecodeError::InvalidPacketId(None) => write!(f, "Invalid packet id"),
            DecodeError::InvalidLength => write!(f, "Invalid length value"),
        }
    }
}

impl std::error::Error for DecodeError {}

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

        assert_eq!(result, Err(DecodeError::InvalidPacketId(Some(255))));
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
            Err(DecodeError::InvalidPacketId(None)) => (),
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

    #[test]
    fn test_fixed_length_vec_literal() {
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct FixedLengthLiteral {
            #[codec(fixed_length = 3)]
            pub data: Vec<u8>,
        }

        let original = FixedLengthLiteral {
            data: vec![1, 2, 3],
        };

        let mut buf = Vec::new();
        original.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = FixedLengthLiteral::decode(&mut slice).unwrap();

        assert_eq!(decoded, original);
    }

    #[test]
    fn test_fixed_length_vec_field_reference() {
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct FixedLengthFieldRef {
            pub length: u32,
            #[codec(fixed_length = length)]
            pub data: Vec<u8>,
        }

        let original = FixedLengthFieldRef {
            length: 5,
            data: vec![10, 20, 30, 40, 50],
        };

        let mut buf = Vec::new();
        original.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = FixedLengthFieldRef::decode(&mut slice).unwrap();

        assert_eq!(decoded, original);
    }

    #[test]
    fn test_fixed_length_vec_arithmetic() {
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct FixedLengthArithmetic {
            pub length: u32,
            pub factor: u32,
            #[codec(fixed_length = length * factor)]
            pub data: Vec<u8>,
        }

        let original = FixedLengthArithmetic {
            length: 3,
            factor: 2,
            data: vec![1, 2, 3, 4, 5, 6],
        };

        let mut buf = Vec::new();
        original.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = FixedLengthArithmetic::decode(&mut slice).unwrap();

        assert_eq!(decoded, original);
    }

    #[test]
    fn test_fixed_length_vec_complex_expression() {
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct FixedLengthComplex {
            pub length: u32,
            pub factor: u32,
            #[codec(fixed_length = (length + 5) * factor - 2)]
            pub data: Vec<u8>,
        }

        // (4 + 5) * 2 - 2 = 9 * 2 - 2 = 18 - 2 = 16
        let original = FixedLengthComplex {
            length: 4,
            factor: 2,
            data: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        };

        let mut buf = Vec::new();
        original.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = FixedLengthComplex::decode(&mut slice).unwrap();

        assert_eq!(decoded, original);
    }

    #[test]
    fn test_fixed_length_vec_multiple_fields() {
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct MultipleFixedLength {
            pub length: u32,
            pub factor: u32,
            #[codec(fixed_length = length)]
            pub field_a: Vec<u8>,
            #[codec(fixed_length = length * factor)]
            pub field_b: Vec<u8>,
            #[codec(fixed_length = 2)]
            pub field_c: Vec<u8>,
        }

        let original = MultipleFixedLength {
            length: 3,
            factor: 2,
            field_a: vec![1, 2, 3],
            field_b: vec![4, 5, 6, 7, 8, 9],
            field_c: vec![10, 11],
        };

        let mut buf = Vec::new();
        original.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = MultipleFixedLength::decode(&mut slice).unwrap();

        assert_eq!(decoded, original);
    }

    #[test]
    #[should_panic(expected = "Fixed-length vector length mismatch")]
    fn test_fixed_length_vec_wrong_length() {
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct FixedLengthWrong {
            #[codec(fixed_length = 3)]
            pub data: Vec<u8>,
        }

        let invalid = FixedLengthWrong {
            data: vec![1, 2], // Wrong: should be 3 items
        };

        let mut buf = Vec::new();
        invalid.encode(&mut buf); // Should panic
    }

    #[test]
    fn test_fixed_length_no_length_prefix() {
        // Verify that fixed-length vectors don't encode a length prefix
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct FixedLengthNormal {
            pub length: u32,
            #[codec(fixed_length = length)]
            pub data: Vec<u8>,
        }

        let original = FixedLengthNormal {
            length: 5,
            data: vec![10, 20, 30, 40, 50],
        };

        let mut buf = Vec::new();
        original.encode(&mut buf);

        // The buffer should contain:
        // - 4 bytes for u32 length field (big-endian): 0x00, 0x00, 0x00, 0x05
        // - 5 bytes for the data (no length prefix): 10, 20, 30, 40, 50
        // Total: 9 bytes (NOT 10 bytes which would include a varint32 length prefix)
        assert_eq!(
            buf.len(),
            9,
            "Fixed-length vector should not include length prefix"
        );
        assert_eq!(buf[0..4], [0, 0, 0, 5]); // The length field
        assert_eq!(buf[4..9], [10, 20, 30, 40, 50]); // The data without prefix
    }

    #[test]
    fn test_fixed_length_vs_normal_vector_size() {
        // Compare sizes: fixed-length should NOT have varint length prefix, normal vectors should
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode)]
        pub struct WithNormalVector {
            pub data: Vec<u8>,
        }

        #[derive(void_codec_macros::Encode, void_codec_macros::Decode)]
        pub struct WithFixedLengthVector {
            pub length: u32,
            #[codec(fixed_length = length)]
            pub data: Vec<u8>,
        }

        let data = vec![1, 2, 3, 4, 5];

        let normal = WithNormalVector { data: data.clone() };

        let fixed = WithFixedLengthVector {
            length: 5,
            data: data.clone(),
        };

        let mut normal_buf = Vec::new();
        normal.encode(&mut normal_buf);

        let mut fixed_buf = Vec::new();
        fixed.encode(&mut fixed_buf);

        // Normal vector: 1 byte varint32 length prefix + 5 bytes data = 6 bytes
        // Fixed vector: 4 bytes length (u32) + 5 bytes data (no prefix) = 9 bytes
        // Note: fixed is larger because it includes u32 field, but its data section is smaller
        assert_eq!(
            normal_buf.len(),
            6,
            "Normal vector should include 1-byte varint32 length prefix"
        );
        assert_eq!(normal_buf[0], 5); // First byte is the varint32 length
        assert_eq!(normal_buf[1..], [1, 2, 3, 4, 5]); // Then the data

        assert_eq!(
            fixed_buf.len(),
            9,
            "Fixed-length vector struct includes u32 field"
        );
        assert_eq!(fixed_buf[0..4], [0, 0, 0, 5]); // u32 length field (big-endian)
        assert_eq!(fixed_buf[4..9], [1, 2, 3, 4, 5]); // Data without varint prefix
    }

    #[test]
    fn test_fixed_length_decode_from_raw_bytes() {
        // Verify we can decode fixed-length vectors from manually constructed byte buffers
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct FixedLengthManual {
            pub length: u32,
            #[codec(fixed_length = length)]
            pub data: Vec<u8>,
        }

        // Manually construct the buffer:
        // - 4 bytes for length (5 in big-endian): 0x00, 0x00, 0x00, 0x05
        // - 5 bytes of data: 100, 101, 102, 103, 104
        let buf = vec![0x00, 0x00, 0x00, 0x05, 100, 101, 102, 103, 104];

        let mut slice = buf.as_slice();
        let decoded = FixedLengthManual::decode(&mut slice).unwrap();

        assert_eq!(decoded.length, 5);
        assert_eq!(decoded.data, vec![100, 101, 102, 103, 104]);
        // All bytes should be consumed
        assert_eq!(slice.len(), 0);
    }

    #[test]
    fn test_fixed_length_roundtrip_preserves_format() {
        // Verify encode -> decode -> encode produces identical bytes
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct FixedLengthRoundtrip {
            pub length: u32,
            #[codec(fixed_length = length)]
            pub data: Vec<u8>,
        }

        let original = FixedLengthRoundtrip {
            length: 3,
            data: vec![42, 43, 44],
        };

        // First encoding
        let mut buf1 = Vec::new();
        original.encode(&mut buf1);

        // Decode
        let mut slice = buf1.as_slice();
        let decoded = FixedLengthRoundtrip::decode(&mut slice).unwrap();

        // Second encoding
        let mut buf2 = Vec::new();
        decoded.encode(&mut buf2);

        // Buffers should be identical
        assert_eq!(
            buf1, buf2,
            "Roundtrip encode/decode should produce identical bytes"
        );
    }

    #[test]
    fn test_fixed_length_multiple_fields_binary_format() {
        // Verify multiple fixed-length fields don't include prefixes
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct MultipleFix {
            pub len_a: u32,
            pub len_b: u32,
            #[codec(fixed_length = len_a)]
            pub field_a: Vec<u8>,
            #[codec(fixed_length = len_b)]
            pub field_b: Vec<u8>,
        }

        let original = MultipleFix {
            len_a: 2,
            len_b: 3,
            field_a: vec![1, 2],
            field_b: vec![10, 11, 12],
        };

        let mut buf = Vec::new();
        original.encode(&mut buf);

        // Expected format:
        // - 4 bytes len_a: 0x00, 0x00, 0x00, 0x02
        // - 4 bytes len_b: 0x00, 0x00, 0x00, 0x03
        // - 2 bytes field_a: 1, 2 (NO prefix)
        // - 3 bytes field_b: 10, 11, 12 (NO prefix)
        // Total: 13 bytes
        assert_eq!(buf.len(), 13);
        assert_eq!(buf[0..4], [0, 0, 0, 2]); // len_a
        assert_eq!(buf[4..8], [0, 0, 0, 3]); // len_b
        assert_eq!(buf[8..10], [1, 2]); // field_a
        assert_eq!(buf[10..13], [10, 11, 12]); // field_b
    }

    #[test]
    fn test_fixed_length_with_expression_binary() {
        // Verify fixed-length with arithmetic expression works correctly
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct FixedWithExpr {
            pub base: u32,
            pub multiplier: u32,
            #[codec(fixed_length = base * multiplier)]
            pub data: Vec<u8>,
        }

        let original = FixedWithExpr {
            base: 3,
            multiplier: 2,
            data: vec![1, 2, 3, 4, 5, 6],
        };

        let mut buf = Vec::new();
        original.encode(&mut buf);

        // Expected format:
        // - 4 bytes base: 0x00, 0x00, 0x00, 0x03
        // - 4 bytes multiplier: 0x00, 0x00, 0x00, 0x02
        // - 6 bytes data: 1, 2, 3, 4, 5, 6 (NO prefix)
        // Total: 14 bytes
        assert_eq!(buf.len(), 14);

        // Verify roundtrip
        let mut slice = buf.as_slice();
        let decoded = FixedWithExpr::decode(&mut slice).unwrap();
        assert_eq!(decoded, original);
        assert_eq!(slice.len(), 0);
    }

    #[test]
    fn test_fixed_length_vec_u8_large_buffer() {
        // Verify optimization for Vec<u8> with large buffers
        // This would be slow if we called u8::decode() 32000 times
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct LargeFixedBuffer {
            pub size: u32,
            #[codec(fixed_length = size)]
            pub data: Vec<u8>,
        }

        // Create a large buffer of 10000 bytes
        let large_data = vec![42u8; 10000];
        let original = LargeFixedBuffer {
            size: 10000,
            data: large_data.clone(),
        };

        let mut buf = Vec::new();
        original.encode(&mut buf);

        // Buffer should be: 4 bytes (u32) + 10000 bytes (data, no prefix)
        assert_eq!(buf.len(), 10004);

        // Decode should be fast (not calling decode 10000 times)
        let mut slice = buf.as_slice();
        let decoded = LargeFixedBuffer::decode(&mut slice).unwrap();

        assert_eq!(decoded.size, 10000);
        assert_eq!(decoded.data, large_data);
        assert_eq!(slice.len(), 0);
    }

    #[test]
    fn test_fixed_length_vec_u8_optimized_vs_generic() {
        // Verify Vec<u8> uses optimized path and generic types use generic path
        #[derive(void_codec_macros::Encode, void_codec_macros::Decode, PartialEq, Debug)]
        pub struct MixedVectors {
            pub len: u32,
            #[codec(fixed_length = len)]
            pub bytes: Vec<u8>,
            #[codec(fixed_length = len)]
            pub numbers: Vec<u16>,
        }

        let original = MixedVectors {
            len: 5,
            bytes: vec![1, 2, 3, 4, 5],
            numbers: vec![100, 101, 102, 103, 104],
        };

        let mut buf = Vec::new();
        original.encode(&mut buf);

        // bytes: 5 bytes (optimized path, no individual decode calls)
        // numbers: 5 * 2 = 10 bytes (generic path with u16::decode calls)
        // len: 4 bytes (u32)
        // Total: 4 + 5 + 10 = 19 bytes
        assert_eq!(buf.len(), 19);

        let mut slice = buf.as_slice();
        let decoded = MixedVectors::decode(&mut slice).unwrap();

        assert_eq!(decoded, original);
        assert_eq!(slice.len(), 0);
    }

    // Tests for #[codec(remaining)] attribute
    #[derive(Encode, Decode, Debug, PartialEq)]
    struct SimpleRemaining {
        id: u32,
        #[codec(remaining)]
        data: Vec<u8>,
    }

    #[test]
    fn test_remaining_basic() {
        let original = SimpleRemaining {
            id: 42,
            data: vec![1, 2, 3, 4, 5],
        };

        let mut buf = Vec::new();
        original.encode(&mut buf);

        // Verify binary format: 4 bytes for id + 5 bytes for data (no length prefix)
        assert_eq!(buf.len(), 9);

        let mut slice = buf.as_slice();
        let decoded = SimpleRemaining::decode(&mut slice).unwrap();

        assert_eq!(decoded, original);
        assert_eq!(slice.len(), 0); // All bytes consumed
    }

    #[test]
    fn test_remaining_empty() {
        let original = SimpleRemaining {
            id: 100,
            data: vec![],
        };

        let mut buf = Vec::new();
        original.encode(&mut buf);

        // Only 4 bytes for id, empty data
        assert_eq!(buf.len(), 4);

        let mut slice = buf.as_slice();
        let decoded = SimpleRemaining::decode(&mut slice).unwrap();

        assert_eq!(decoded, original);
        assert_eq!(slice.len(), 0);
    }

    #[test]
    fn test_remaining_large_data() {
        let data = vec![42u8; 10000];
        let original = SimpleRemaining {
            id: 999,
            data: data.clone(),
        };

        let mut buf = Vec::new();
        original.encode(&mut buf);

        // 4 bytes for id + 10000 bytes for data (no length prefix)
        assert_eq!(buf.len(), 10004);

        let mut slice = buf.as_slice();
        let decoded = SimpleRemaining::decode(&mut slice).unwrap();

        assert_eq!(decoded, original);
        assert_eq!(slice.len(), 0);
    }

    #[test]
    fn test_remaining_consumes_all_bytes() {
        let original = SimpleRemaining {
            id: 1,
            data: vec![10, 20, 30],
        };

        let mut buf = Vec::new();
        original.encode(&mut buf);

        // Add some extra garbage after encoding
        buf.push(255);
        buf.push(254);
        buf.push(253);

        let mut slice = buf.as_slice();
        let decoded = SimpleRemaining::decode(&mut slice).unwrap();

        // After decode, remaining should be empty (remaining attribute consumed garbage too)
        assert_eq!(slice.len(), 0);

        // The decoded data field should include the garbage
        assert_eq!(decoded.data, vec![10, 20, 30, 255, 254, 253]);
    }

    #[test]
    fn test_remaining_no_length_prefix() {
        let original = SimpleRemaining {
            id: 5,
            data: vec![100, 101, 102],
        };

        let mut buf = Vec::new();
        original.encode(&mut buf);

        // Manual verification: no varint32 length prefix before data
        // id (u32) = 4 bytes (big-endian): [0, 0, 0, 5]
        // data (3 bytes, no prefix): [100, 101, 102]
        // Total: 7 bytes
        assert_eq!(buf[0..4], [0, 0, 0, 5]);
        assert_eq!(buf[4..7], [100, 101, 102]);
        assert_eq!(buf.len(), 7);
    }

    #[test]
    fn test_remaining_with_string_field() {
        #[derive(Encode, Decode, Debug, PartialEq)]
        struct MessageWithData {
            channel: String,
            #[codec(remaining)]
            payload: Vec<u8>,
        }

        let original = MessageWithData {
            channel: "test_channel".to_string(),
            payload: vec![1, 2, 3, 4],
        };

        let mut buf = Vec::new();
        original.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = MessageWithData::decode(&mut slice).unwrap();

        assert_eq!(decoded, original);
        assert_eq!(slice.len(), 0);
    }

    #[test]
    fn test_remaining_multiple_u8_fields() {
        #[derive(Encode, Decode, Debug, PartialEq)]
        struct MultiField {
            byte1: u8,
            byte2: u8,
            #[codec(remaining)]
            rest: Vec<u8>,
        }

        let original = MultiField {
            byte1: 10,
            byte2: 20,
            rest: vec![30, 40, 50, 60],
        };

        let mut buf = Vec::new();
        original.encode(&mut buf);

        // 1 + 1 + 4 = 6 bytes (no length prefix on rest)
        assert_eq!(buf.len(), 6);

        let mut slice = buf.as_slice();
        let decoded = MultiField::decode(&mut slice).unwrap();

        assert_eq!(decoded, original);
        assert_eq!(slice.len(), 0);
    }

    #[test]
    fn test_remaining_preserves_all_bytes() {
        let data = b"Plugin message with binary \x00\x01\x02\x03 data".to_vec();
        let original = SimpleRemaining {
            id: 777,
            data: data.clone(),
        };

        let mut buf = Vec::new();
        original.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = SimpleRemaining::decode(&mut slice).unwrap();

        // Verify exact byte preservation (including null bytes)
        assert_eq!(decoded.data, data);
        assert_eq!(slice.len(), 0);
    }

    #[test]
    fn test_remaining_roundtrip_multiple_times() {
        let test_cases = vec![
            vec![],
            vec![1],
            vec![1, 2, 3, 4, 5],
            vec![255, 254, 253],
            vec![0; 100],
        ];

        for data in test_cases {
            let original = SimpleRemaining {
                id: 42,
                data: data.clone(),
            };

            let mut buf = Vec::new();
            original.encode(&mut buf);

            let mut slice = buf.as_slice();
            let decoded = SimpleRemaining::decode(&mut slice).unwrap();

            assert_eq!(decoded, original, "Roundtrip failed for data: {:?}", data);
            assert_eq!(slice.len(), 0);

            // Re-encode and verify same result
            let mut buf2 = Vec::new();
            decoded.encode(&mut buf2);
            assert_eq!(buf, buf2, "Encode changed for data: {:?}", data);
        }
    }

    #[test]
    fn test_remaining_vs_fixed_length_size_difference() {
        // fixed_length requires length to be pre-known and doesn't store it
        // remaining consumes all remaining bytes
        // For fixed_length, we need to encode both the length and the data
        // For remaining, we only encode the data

        #[derive(Encode, Decode, Debug, PartialEq)]
        struct WithFixedKnownLen {
            #[codec(fixed_length = 4)]
            data: Vec<u8>,
        }

        #[derive(Encode, Decode, Debug, PartialEq)]
        struct WithRemaining {
            #[codec(remaining)]
            data: Vec<u8>,
        }

        let data = vec![1, 2, 3, 4];

        let fixed = WithFixedKnownLen { data: data.clone() };

        let remaining = WithRemaining { data: data.clone() };

        let mut buf_fixed = Vec::new();
        fixed.encode(&mut buf_fixed);

        let mut buf_remaining = Vec::new();
        remaining.encode(&mut buf_remaining);

        // Both should be same size (4 bytes) since fixed_length = 4 means no prefix
        // and remaining also has no prefix
        assert_eq!(buf_fixed.len(), buf_remaining.len());
        assert_eq!(buf_fixed, buf_remaining);
    }
}
