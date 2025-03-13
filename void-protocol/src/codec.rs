use std::io::Write;

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
}

impl PacketEncode for Vec<u8> {}

#[cfg(test)]
mod tests {
    use super::PacketEncode;

    #[test]
    fn test_encode_u8() {
        let mut buffer = Vec::new();
        buffer.encode_u8(0x12).expect("Encoding failed");
        assert_eq!(buffer, vec![0x12]);
    }

    #[test]
    fn test_encode_u16() {
        let mut buffer = Vec::new();
        buffer.encode_u16(0x1234).expect("Encoding failed");
        assert_eq!(buffer, vec![0x12, 0x34]);
    }

    #[test]
    fn test_encode_u32() {
        let mut buffer = Vec::new();
        buffer.encode_u32(0x12345678).expect("Encoding failed");
        assert_eq!(buffer, vec![0x12, 0x34, 0x56, 0x78]);
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
    fn test_encode_i8() {
        let mut buffer = Vec::new();
        buffer.encode_i8(-0x12).expect("Encoding failed");
        assert_eq!(buffer, vec![0xee]);
    }

    #[test]
    fn test_encode_i16() {
        let mut buffer = Vec::new();
        buffer.encode_i16(-0x1234).expect("Encoding failed");
        assert_eq!(buffer, vec![0xed, 0xcc]);
    }

    #[test]
    fn test_encode_i32() {
        let mut buffer = Vec::new();
        buffer.encode_i32(-0x12345678).expect("Encoding failed");
        assert_eq!(buffer, vec![0xed, 0xcb, 0xa9, 0x88]);
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
}
