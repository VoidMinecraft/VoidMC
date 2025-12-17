use crate::{Decode, DecodeError, Encode};

impl Encode for i32 {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_be_bytes());
    }
}

impl Decode for i32 {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        if buf.len() < 4 {
            return Err(DecodeError::UnexpectedEof);
        }

        let (bytes, rest) = buf.split_at(4);
        *buf = rest;

        let mut array = [0u8; 4];
        array.copy_from_slice(bytes);
        Ok(i32::from_be_bytes(array))
    }
}

impl Encode for u8 {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.push(*self);
    }
}

impl Decode for u8 {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        if buf.is_empty() {
            return Err(DecodeError::UnexpectedEof);
        }

        let value = buf[0];
        *buf = &buf[1..];
        Ok(value)
    }
}

impl Encode for i64 {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_be_bytes());
    }
}

impl Decode for i64 {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        if buf.len() < 8 {
            return Err(DecodeError::UnexpectedEof);
        }

        let (bytes, rest) = buf.split_at(8);
        *buf = rest;

        let mut array = [0u8; 8];
        array.copy_from_slice(bytes);
        Ok(i64::from_be_bytes(array))
    }
}

impl Encode for u64 {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_be_bytes());
    }
}

impl Decode for u64 {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        if buf.len() < 8 {
            return Err(DecodeError::UnexpectedEof);
        }

        let (bytes, rest) = buf.split_at(8);
        *buf = rest;

        let mut array = [0u8; 8];
        array.copy_from_slice(bytes);
        Ok(u64::from_be_bytes(array))
    }
}

impl Encode for u32 {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_be_bytes());
    }
}

impl Decode for u32 {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        if buf.len() < 4 {
            return Err(DecodeError::UnexpectedEof);
        }

        let (bytes, rest) = buf.split_at(4);
        *buf = rest;

        let mut array = [0u8; 4];
        array.copy_from_slice(bytes);
        Ok(u32::from_be_bytes(array))
    }
}

impl Encode for u16 {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_be_bytes());
    }
}

impl Decode for u16 {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        if buf.len() < 2 {
            return Err(DecodeError::UnexpectedEof);
        }

        let (bytes, rest) = buf.split_at(2);
        *buf = rest;

        let mut array = [0u8; 2];
        array.copy_from_slice(bytes);
        Ok(u16::from_be_bytes(array))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_i32_big_endian() {
        let value = 12345i32;
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf.len(), 4);

        let mut slice = buf.as_slice();
        let decoded = i32::decode(&mut slice).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_u8_single_byte() {
        let value = 255u8;
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf, vec![255]);

        let mut slice = buf.as_slice();
        let decoded = u8::decode(&mut slice).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_u64_roundtrip() {
        let value = 9223372036854775807u64;
        let mut buf = Vec::new();
        value.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = u64::decode(&mut slice).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_i64_big_endian() {
        let value = -123456i64;
        let mut buf = Vec::new();
        value.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = i64::decode(&mut slice).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_u32_roundtrip() {
        let value = 4294967295u32;
        let mut buf = Vec::new();
        value.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = u32::decode(&mut slice).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_i32_exact_bytes() {
        let value: i32 = 0x12345678;
        let mut buf = Vec::new();
        value.encode(&mut buf);

        assert_eq!(buf, vec![0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn test_i32_zero() {
        let mut buf = Vec::new();
        (0i32).encode(&mut buf);
        assert_eq!(buf, vec![0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_i32_negative_one() {
        let mut buf = Vec::new();
        (-1i32).encode(&mut buf);
        assert_eq!(buf, vec![0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn test_i32_max() {
        let mut buf = Vec::new();
        (i32::MAX).encode(&mut buf);
        assert_eq!(buf, vec![0x7F, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn test_i32_min() {
        let mut buf = Vec::new();
        (i32::MIN).encode(&mut buf);
        assert_eq!(buf, vec![0x80, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_u8_exact_bytes() {
        let values = [0u8, 1, 127, 128, 255];
        for value in &values {
            let mut buf = Vec::new();
            value.encode(&mut buf);
            assert_eq!(buf, vec![*value]);
        }
    }

    #[test]
    fn test_i64_exact_bytes() {
        let value: i64 = 0x0102030405060708;
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf, vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]);
    }

    #[test]
    fn test_i64_negative_one() {
        let mut buf = Vec::new();
        (-1i64).encode(&mut buf);
        assert_eq!(buf, vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn test_i64_max() {
        let mut buf = Vec::new();
        (i64::MAX).encode(&mut buf);
        assert_eq!(buf, vec![0x7F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn test_i64_min() {
        let mut buf = Vec::new();
        (i64::MIN).encode(&mut buf);
        assert_eq!(buf, vec![0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_u32_exact_bytes() {
        let value: u32 = 0xAABBCCDD;
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf, vec![0xAA, 0xBB, 0xCC, 0xDD]);
    }

    #[test]
    fn test_u32_zero() {
        let mut buf = Vec::new();
        (0u32).encode(&mut buf);
        assert_eq!(buf, vec![0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_u32_max() {
        let mut buf = Vec::new();
        (u32::MAX).encode(&mut buf);
        assert_eq!(buf, vec![0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn test_u64_exact_bytes() {
        let value: u64 = 0x0102030405060708;
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf, vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]);
    }

    #[test]
    fn test_u64_zero() {
        let mut buf = Vec::new();
        (0u64).encode(&mut buf);
        assert_eq!(buf, vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_u64_max() {
        let mut buf = Vec::new();
        (u64::MAX).encode(&mut buf);
        assert_eq!(buf, vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn test_u16_roundtrip() {
        let value = 12345u16;
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf.len(), 2);

        let mut slice = buf.as_slice();
        let decoded = u16::decode(&mut slice).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_u16_exact_bytes() {
        let value = 0x1234u16;
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf, vec![0x12, 0x34]);
    }

    #[test]
    fn test_u16_zero() {
        let mut buf = Vec::new();
        (0u16).encode(&mut buf);
        assert_eq!(buf, vec![0x00, 0x00]);
    }

    #[test]
    fn test_u16_max() {
        let mut buf = Vec::new();
        (u16::MAX).encode(&mut buf);
        assert_eq!(buf, vec![0xFF, 0xFF]);
    }
}
