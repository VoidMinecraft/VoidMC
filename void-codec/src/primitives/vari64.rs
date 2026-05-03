use crate::{Decode, DecodeError, Encode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarI64(pub i64);

impl Encode for VarI64 {
    fn encode(&self, buf: &mut Vec<u8>) {
        let mut value = self.0 as u64;
        loop {
            let mut byte = (value & 0x7F) as u8;
            value >>= 7;

            if value != 0 {
                byte |= 0x80;
            }

            buf.push(byte);

            if value == 0 {
                break;
            }
        }
    }
}

impl Decode for VarI64 {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let mut value: u64 = 0;
        let mut shift = 0;

        for _ in 0..10 {
            if buf.is_empty() {
                return Err(DecodeError::UnexpectedEof);
            }

            let byte = buf[0];
            *buf = &buf[1..];

            value |= ((byte & 0x7F) as u64) << shift;

            if byte & 0x80 == 0 {
                return Ok(VarI64(value as i64));
            }

            shift += 7;
        }

        Err(DecodeError::InvalidVarintLength)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vari64_zero() {
        let value = VarI64(0);
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf, vec![0x00]);

        let mut slice = buf.as_slice();
        let decoded = VarI64::decode(&mut slice).unwrap();
        assert_eq!(decoded.0, 0);
    }

    #[test]
    fn test_vari64_small_positive() {
        let value = VarI64(127);
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf, vec![0x7F]);

        let mut slice = buf.as_slice();
        let decoded = VarI64::decode(&mut slice).unwrap();
        assert_eq!(decoded.0, 127);
    }

    #[test]
    fn test_vari64_128() {
        let value = VarI64(128);
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf, vec![0x80, 0x01]);

        let mut slice = buf.as_slice();
        let decoded = VarI64::decode(&mut slice).unwrap();
        assert_eq!(decoded.0, 128);
    }

    #[test]
    fn test_vari64_negative() {
        let value = VarI64(-1);
        let mut buf = Vec::new();
        value.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = VarI64::decode(&mut slice).unwrap();
        assert_eq!(decoded.0, -1);
    }

    #[test]
    fn test_vari64_max() {
        let value = VarI64(i64::MAX);
        let mut buf = Vec::new();
        value.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = VarI64::decode(&mut slice).unwrap();
        assert_eq!(decoded.0, i64::MAX);
    }

    #[test]
    fn test_vari64_min() {
        let value = VarI64(i64::MIN);
        let mut buf = Vec::new();
        value.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = VarI64::decode(&mut slice).unwrap();
        assert_eq!(decoded.0, i64::MIN);
    }

    #[test]
    fn test_vari64_truncated() {
        let mut slice = &[0x80][..];
        let result = VarI64::decode(&mut slice);
        assert_eq!(result, Err(DecodeError::UnexpectedEof));
    }

    #[test]
    fn test_vari64_too_long() {
        let bytes = vec![
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        ];
        let mut slice = bytes.as_slice();
        let result = VarI64::decode(&mut slice);
        assert_eq!(result, Err(DecodeError::InvalidVarintLength));
    }

    #[test]
    fn test_vari64_exact_bytes_zero() {
        let value = VarI64(0);
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf, vec![0x00]);
    }

    #[test]
    fn test_vari64_exact_bytes_one() {
        let value = VarI64(1);
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf, vec![0x01]);
    }

    #[test]
    fn test_vari64_exact_bytes_127() {
        let value = VarI64(127);
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf, vec![0x7F]);
    }

    #[test]
    fn test_vari64_exact_bytes_128() {
        let value = VarI64(128);
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf, vec![0x80, 0x01]);
    }

    #[test]
    fn test_vari64_exact_bytes_16384() {
        let value = VarI64(16384);
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf, vec![0x80, 0x80, 0x01]);
    }

    #[test]
    fn test_vari64_negative_exact_bytes() {
        let value = VarI64(-1);
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(
            buf,
            vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01]
        );
    }

    #[test]
    fn test_vari64_negative_two() {
        let value = VarI64(-2);
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(
            buf,
            vec![0xFE, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01]
        );
    }

    #[test]
    fn test_vari64_100() {
        let value = VarI64(100);
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf.len(), 1);
        assert_eq!(buf, vec![100]);
    }

    #[test]
    fn test_vari64_1000() {
        let value = VarI64(1000);
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf.len(), 2);
        assert_eq!(buf, vec![0xE8, 0x07]);
    }

    #[test]
    fn test_vari64_serialization_efficiency() {
        struct TestCase {
            value: i64,
            expected_len: usize,
        }

        let cases = [
            TestCase {
                value: 0,
                expected_len: 1,
            },
            TestCase {
                value: 127,
                expected_len: 1,
            },
            TestCase {
                value: 128,
                expected_len: 2,
            },
            TestCase {
                value: 16383,
                expected_len: 2,
            },
            TestCase {
                value: 16384,
                expected_len: 3,
            },
            TestCase {
                value: 2097151,
                expected_len: 3,
            },
            TestCase {
                value: 2097152,
                expected_len: 4,
            },
            TestCase {
                value: i64::MAX,
                expected_len: 9,
            },
            TestCase {
                value: i64::MIN,
                expected_len: 10,
            },
        ];

        for case in &cases {
            let mut buf = Vec::new();
            VarI64(case.value).encode(&mut buf);
            assert_eq!(
                buf.len(),
                case.expected_len,
                "VarI64({}) should encode to {} bytes, got {}",
                case.value,
                case.expected_len,
                buf.len()
            );
        }
    }

    #[test]
    fn test_vari64_java_compliance_roundtrip() {
        let test_values = [
            0i64,
            1,
            127,
            128,
            255,
            256,
            16383,
            16384,
            2097151,
            2097152,
            -1,
            -2,
            -128,
            -32768,
            i64::MAX,
            i64::MIN,
        ];

        for value in &test_values {
            let vi64 = VarI64(*value);
            let mut buf = Vec::new();
            vi64.encode(&mut buf);

            let mut slice = buf.as_slice();
            let decoded = VarI64::decode(&mut slice).unwrap();
            assert_eq!(
                decoded.0, *value,
                "VarI64 roundtrip failed for value {}",
                value
            );
            assert_eq!(
                slice.len(),
                0,
                "VarI64 decode didn't consume all bytes for value {}",
                value
            );
        }
    }

    #[test]
    fn test_vari64_max_bytes_limit() {
        let mut buf = Vec::new();
        (i64::MAX).encode(&mut buf);
        assert!(
            buf.len() <= 10,
            "VarI64 should encode to at most 10 bytes, got {}",
            buf.len()
        );
    }

    #[test]
    fn test_vari64_more_than_ten_bytes_rejected() {
        let bytes = vec![
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01,
        ];
        let mut slice = bytes.as_slice();
        let result = VarI64::decode(&mut slice);
        assert!(
            result.is_err(),
            "VarI64 with 11 bytes should be rejected like Java's position >= 64 check"
        );
    }

    #[test]
    fn test_vari64_java_segment_and_continue_bits() {
        let value = VarI64(300);
        let mut buf = Vec::new();
        value.encode(&mut buf);

        assert_eq!(buf.len(), 2);
        assert_eq!(buf[0] & 0x80, 0x80, "First byte should have continue bit");
        assert_eq!(
            buf[1] & 0x80,
            0x00,
            "Last byte should NOT have continue bit"
        );

        let segment_bits = 0x7F;

        assert_eq!(
            buf[0] & segment_bits,
            300u64 as u8 & segment_bits,
            "First byte segment bits should match"
        );
    }

    #[test]
    fn test_vari64_java_logic_equivalence() {
        let test_values = [
            0i64, 1, 63, 64, 127, 128, 255, 256, 16383, 16384, 2097151, 2097152,
        ];

        for &value in &test_values {
            let vi64 = VarI64(value);
            let mut buf = Vec::new();
            vi64.encode(&mut buf);

            assert!(!buf.is_empty(), "VarI64 should encode to at least 1 byte");
            assert!(buf.len() <= 10, "VarI64 should encode to at most 10 bytes");

            let mut decoded_value = 0u64;
            for (i, &byte) in buf.iter().enumerate() {
                decoded_value |= ((byte & 0x7F) as u64) << (i * 7);

                if byte & 0x80 == 0 {
                    assert_eq!(
                        i + 1,
                        buf.len(),
                        "No-continue-bit should be on the last byte"
                    );
                    break;
                }
            }

            assert_eq!(
                decoded_value, value as u64,
                "Manual decode should match encoded value for {}: bytes = {:?}",
                value, buf
            );
        }
    }

    #[test]
    fn test_vari64_large_values() {
        let test_values = [
            1000000i64,
            10000000,
            100000000,
            1000000000,
            10000000000,
            100000000000,
        ];

        for value in &test_values {
            let vi64 = VarI64(*value);
            let mut buf = Vec::new();
            vi64.encode(&mut buf);

            let mut slice = buf.as_slice();
            let decoded = VarI64::decode(&mut slice).unwrap();
            assert_eq!(decoded.0, *value, "Failed for large value {}", value);
            assert!(
                buf.len() <= 10,
                "Large value {} should encode to at most 10 bytes, got {}",
                value,
                buf.len()
            );
        }
    }
}
