use std::io::{Read, Write};

const SEGMENT_BITS_U32: u32 = 0x7F;
const SEGMENT_BITS_U64: u64 = 0x7F;
const CONTINUE_BIT_U32: u32 = 0x80;
const CONTINUE_BIT_U64: u64 = 0x80;

pub trait PacketEncode: Write {
    fn encode_u8(&mut self, value: u8) -> std::io::Result<()> {
        self.write_all(&value.to_be_bytes())
    }

    fn encode_u16(&mut self, value: u16) -> std::io::Result<()> {
        self.write_all(&value.to_be_bytes())
    }

    fn encode_u32(&mut self, value: u32) -> std::io::Result<()> {
        self.write_all(&value.to_be_bytes())
    }

    fn encode_u64(&mut self, value: u64) -> std::io::Result<()> {
        self.write_all(&value.to_be_bytes())
    }

    fn encode_u128(&mut self, value: u128) -> std::io::Result<()> {
        self.write_all(&value.to_be_bytes())
    }

    fn encode_i8(&mut self, value: i8) -> std::io::Result<()> {
        self.write_all(&value.to_be_bytes())
    }

    fn encode_i16(&mut self, value: i16) -> std::io::Result<()> {
        self.write_all(&value.to_be_bytes())
    }

    fn encode_i32(&mut self, value: i32) -> std::io::Result<()> {
        self.write_all(&value.to_be_bytes())
    }

    fn encode_i64(&mut self, value: i64) -> std::io::Result<()> {
        self.write_all(&value.to_be_bytes())
    }

    fn encode_i128(&mut self, value: i128) -> std::io::Result<()> {
        self.write_all(&value.to_be_bytes())
    }

    fn encode_bool(&mut self, value: bool) -> std::io::Result<()> {
        self.encode_u8(if value { 1 } else { 0 })
    }

    fn encode_vari32(&mut self, value: i32) -> std::io::Result<()> {
        let mut value = value as u32;

        loop {
            if (value & !SEGMENT_BITS_U32) == 0 {
                return self.write_all(&[value as u8]);
            }
            self.write_all(&[((value & SEGMENT_BITS_U32) | CONTINUE_BIT_U32) as u8])?;
            value >>= 7;
        }
    }

    fn encode_vari64(&mut self, value: i64) -> std::io::Result<()> {
        let mut value = value as u64;

        loop {
            if (value & !SEGMENT_BITS_U64) == 0 {
                return self.write_all(&[value as u8]);
            }
            self.write_all(&[((value & SEGMENT_BITS_U64) | CONTINUE_BIT_U64) as u8])?;
            value >>= 7;
        }
    }

    fn encode_str(&mut self, value: &str) -> std::io::Result<()> {
        self.encode_vari32(value.len() as i32)?;
        self.write_all(value.as_bytes())
    }
}

impl PacketEncode for Vec<u8> {}

pub trait PacketDecode: Read {
    fn decode_u8(&mut self) -> std::io::Result<u8> {
        let mut buffer = [0; 1];
        self.read_exact(&mut buffer)?;
        Ok(u8::from_be_bytes(buffer))
    }

    fn decode_u16(&mut self) -> std::io::Result<u16> {
        let mut buffer = [0; 2];
        self.read_exact(&mut buffer)?;
        Ok(u16::from_be_bytes(buffer))
    }

    fn decode_u32(&mut self) -> std::io::Result<u32> {
        let mut buffer = [0; 4];
        self.read_exact(&mut buffer)?;
        Ok(u32::from_be_bytes(buffer))
    }

    fn decode_u64(&mut self) -> std::io::Result<u64> {
        let mut buffer = [0; 8];
        self.read_exact(&mut buffer)?;
        Ok(u64::from_be_bytes(buffer))
    }

    fn decode_u128(&mut self) -> std::io::Result<u128> {
        let mut buffer = [0; 16];
        self.read_exact(&mut buffer)?;
        Ok(u128::from_be_bytes(buffer))
    }

    fn decode_i8(&mut self) -> std::io::Result<i8> {
        let mut buffer = [0; 1];
        self.read_exact(&mut buffer)?;
        Ok(i8::from_be_bytes(buffer))
    }

    fn decode_i16(&mut self) -> std::io::Result<i16> {
        let mut buffer = [0; 2];
        self.read_exact(&mut buffer)?;
        Ok(i16::from_be_bytes(buffer))
    }

    fn decode_i32(&mut self) -> std::io::Result<i32> {
        let mut buffer = [0; 4];
        self.read_exact(&mut buffer)?;
        Ok(i32::from_be_bytes(buffer))
    }

    fn decode_i64(&mut self) -> std::io::Result<i64> {
        let mut buffer = [0; 8];
        self.read_exact(&mut buffer)?;
        Ok(i64::from_be_bytes(buffer))
    }

    fn decode_i128(&mut self) -> std::io::Result<i128> {
        let mut buffer = [0; 16];
        self.read_exact(&mut buffer)?;
        Ok(i128::from_be_bytes(buffer))
    }

    fn decode_bool(&mut self) -> std::io::Result<bool> {
        Ok(self.decode_u8()? != 0)
    }
}

impl PacketDecode for &[u8] {}

#[cfg(test)]
mod tests {
    use super::{PacketDecode, PacketEncode};

    #[test]
    fn test_encode_u8() {
        let mut buffer = Vec::new();
        buffer.encode_u8(0x12).expect("Encoding failed");
        assert_eq!(buffer, vec![0x12]);
    }

    #[test]
    fn test_decode_u8() {
        let mut buffer: &[u8] = &[0x12];
        assert_eq!(buffer.decode_u8().expect("Decoding failed"), 0x12);
    }

    #[test]
    fn test_encode_u16() {
        let mut buffer = Vec::new();
        buffer.encode_u16(0x1234).expect("Encoding failed");
        assert_eq!(buffer, vec![0x12, 0x34]);
    }

    #[test]
    fn test_decode_u16() {
        let mut buffer: &[u8] = &[0x12, 0x34];
        assert_eq!(buffer.decode_u16().expect("Decoding failed"), 0x1234);
    }

    #[test]
    fn test_encode_u32() {
        let mut buffer = Vec::new();
        buffer.encode_u32(0x12345678).expect("Encoding failed");
        assert_eq!(buffer, vec![0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn test_decode_u32() {
        let mut buffer: &[u8] = &[0x12, 0x34, 0x56, 0x78];
        assert_eq!(buffer.decode_u32().expect("Decoding failed"), 0x12345678);
    }

    #[test]
    fn test_encode_u64() {
        let mut buffer = Vec::new();
        buffer
            .encode_u64(0x1234567890abcdef)
            .expect("Encoding failed");
        assert_eq!(buffer, vec![0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef]);
    }

    #[test]
    fn test_decode_u64() {
        let mut buffer: &[u8] = &[0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef];
        assert_eq!(
            buffer.decode_u64().expect("Decoding failed"),
            0x1234567890abcdef
        );
    }

    #[test]
    fn test_encode_u128() {
        let mut buffer = Vec::new();
        buffer
            .encode_u128(0x1234567890abcdef1234567890abcdef)
            .expect("Encoding failed");
        assert_eq!(
            buffer,
            vec![
                0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab,
                0xcd, 0xef,
            ]
        );
    }

    #[test]
    fn test_decode_u128() {
        let mut buffer: &[u8] = &[
            0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab,
            0xcd, 0xef,
        ];
        assert_eq!(
            buffer.decode_u128().expect("Decoding failed"),
            0x1234567890abcdef1234567890abcdef
        );
    }

    #[test]
    fn test_encode_i8() {
        let mut buffer = Vec::new();
        buffer.encode_i8(-0x12).expect("Encoding failed");
        assert_eq!(buffer, vec![0xee]);
    }

    #[test]
    fn test_decode_i8() {
        let mut buffer: &[u8] = &[0xee];
        assert_eq!(buffer.decode_i8().expect("Decoding failed"), -0x12);
    }

    #[test]
    fn test_encode_i16() {
        let mut buffer = Vec::new();
        buffer.encode_i16(-0x1234).expect("Encoding failed");
        assert_eq!(buffer, vec![0xed, 0xcc]);
    }

    #[test]
    fn test_decode_i16() {
        let mut buffer: &[u8] = &[0xed, 0xcc];
        assert_eq!(buffer.decode_i16().expect("Decoding failed"), -0x1234);
    }

    #[test]
    fn test_encode_i32() {
        let mut buffer = Vec::new();
        buffer.encode_i32(-0x12345678).expect("Encoding failed");
        assert_eq!(buffer, vec![0xed, 0xcb, 0xa9, 0x88]);
    }

    #[test]
    fn test_decode_i32() {
        let mut buffer: &[u8] = &[0xed, 0xcb, 0xa9, 0x88];
        assert_eq!(buffer.decode_i32().expect("Decoding failed"), -0x12345678);
    }

    #[test]
    fn test_encode_i64() {
        let mut buffer = Vec::new();
        buffer
            .encode_i64(-0x1234567890abcdef)
            .expect("Encoding failed");
        assert_eq!(buffer, vec![0xed, 0xcb, 0xa9, 0x87, 0x6f, 0x54, 0x32, 0x11]);
    }

    #[test]
    fn test_decode_i64() {
        let mut buffer: &[u8] = &[0xed, 0xcb, 0xa9, 0x87, 0x6f, 0x54, 0x32, 0x11];
        assert_eq!(
            buffer.decode_i64().expect("Decoding failed"),
            -0x1234567890abcdef
        );
    }

    #[test]
    fn test_encode_i128() {
        let mut buffer = Vec::new();
        buffer
            .encode_i128(-0x1234567890abcdef1234567890abcdef)
            .expect("Encoding failed");
        assert_eq!(
            buffer,
            vec![
                0xed, 0xcb, 0xa9, 0x87, 0x6f, 0x54, 0x32, 0x10, 0xed, 0xcb, 0xa9, 0x87, 0x6f, 0x54,
                0x32, 0x11,
            ]
        );
    }

    #[test]
    fn test_decode_i128() {
        let mut buffer: &[u8] = &[
            0xed, 0xcb, 0xa9, 0x87, 0x6f, 0x54, 0x32, 0x10, 0xed, 0xcb, 0xa9, 0x87, 0x6f, 0x54,
            0x32, 0x11,
        ];
        assert_eq!(
            buffer.decode_i128().expect("Decoding failed"),
            -0x1234567890abcdef1234567890abcdef
        );
    }

    #[test]
    fn test_encode_bool() {
        let mut buffer = Vec::new();
        buffer.encode_bool(true).expect("Encoding failed");
        assert_eq!(buffer, vec![0x01]);
        buffer.clear();
        buffer.encode_bool(false).expect("Encoding failed");
        assert_eq!(buffer, vec![0x00]);
    }

    #[test]
    fn test_decode_bool() {
        let mut buffer: &[u8] = &[0x01];
        assert_eq!(buffer.decode_bool().expect("Decoding failed"), true);
        buffer = &[0x00];
        assert_eq!(buffer.decode_bool().expect("Decoding failed"), false);
    }

    #[test]
    fn test_encode_vari32() {
        let mut buffer = Vec::new();
        buffer.encode_vari32(0x12345678).expect("Encoding failed");
        assert_eq!(buffer, vec![0xf8, 0xac, 0xd1, 0x91, 0x01]);
    }

    #[test]
    fn test_encode_vari64() {
        let mut buffer = Vec::new();
        buffer
            .encode_vari64(0x1234567890abcdef)
            .expect("Encoding failed");
        assert_eq!(
            buffer,
            vec![0xef, 0x9b, 0xaf, 0x85, 0x89, 0xcf, 0x95, 0x9a, 0x12]
        );
    }

    #[test]
    fn test_encode_str() {
        let mut buffer = Vec::new();
        buffer.encode_str("Hello, World!").expect("Encoding failed");
        assert_eq!(
            buffer,
            vec![
                0x0d, 0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x2c, 0x20, 0x57, 0x6f, 0x72, 0x6c, 0x64, 0x21
            ]
        );
    }
}
