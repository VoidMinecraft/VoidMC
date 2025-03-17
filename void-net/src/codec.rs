use async_trait::async_trait;
use std::io::{Read, Write};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

const SEGMENT_BITS_U32: u32 = 0x7F;
const SEGMENT_BITS_U64: u64 = 0x7F;
const CONTINUE_BIT_U32: u32 = 0x80;
const CONTINUE_BIT_U64: u64 = 0x80;

/// A trait for encoding various data types into a byte stream.
///
/// This trait extends the `Write` trait and provides methods for encoding
/// different primitive types and strings into a byte stream, according to
/// the Minecraft protocol.
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

    fn encode_f32(&mut self, value: f32) -> std::io::Result<()> {
        self.write_all(&value.to_be_bytes())
    }

    fn encode_f64(&mut self, value: f64) -> std::io::Result<()> {
        self.write_all(&value.to_be_bytes())
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

/// A trait for encoding various data types into a byte stream asynchronously.
///
/// This trait extends the `AsyncWriteExt` trait and provides methods for
/// encoding vari32 into a byte stream, according to the Minecraft protocol.
#[async_trait]
pub trait AsyncPacketEncode: AsyncWriteExt + Unpin {
    async fn encode_vari32(&mut self, value: i32) -> std::io::Result<()> {
        let mut value = value as u32;

        loop {
            if (value & !SEGMENT_BITS_U32) == 0 {
                return self.write_all(&[value as u8]).await;
            }
            self.write_all(&[((value & SEGMENT_BITS_U32) | CONTINUE_BIT_U32) as u8])
                .await?;
            value >>= 7;
        }
    }
}

#[async_trait]
impl AsyncPacketEncode for TcpStream {}

/// A trait for decoding various data types from a byte stream.
///
/// This trait extends the `Read` trait and provides methods for decoding
/// different primitive types and strings from a byte stream, according
/// to the Minecraft protocol.
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

    fn decode_f32(&mut self) -> std::io::Result<f32> {
        let mut buffer = [0; 4];
        self.read_exact(&mut buffer)?;
        Ok(f32::from_be_bytes(buffer))
    }

    fn decode_f64(&mut self) -> std::io::Result<f64> {
        let mut buffer = [0; 8];
        self.read_exact(&mut buffer)?;
        Ok(f64::from_be_bytes(buffer))
    }

    fn decode_bool(&mut self) -> std::io::Result<bool> {
        Ok(self.decode_u8()? != 0)
    }

    fn decode_vari32(&mut self) -> std::io::Result<i32> {
        let mut value: u32 = 0;
        let mut shift: u8 = 0;
        let mut current_byte = [0u8; 1];

        loop {
            self.read_exact(&mut current_byte)?;
            let current_byte: u32 = current_byte[0].into();
            value |= (current_byte & SEGMENT_BITS_U32) << shift;

            if (current_byte & CONTINUE_BIT_U32) == 0 {
                break;
            }

            shift += 7;
            if shift >= 32 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "VarInt too big",
                ));
            }
        }

        Ok(value as i32)
    }

    fn decode_vari64(&mut self) -> std::io::Result<i64> {
        let mut value: u64 = 0;
        let mut shift: u8 = 0;
        let mut current_byte = [0u8; 1];

        loop {
            self.read_exact(&mut current_byte)?;
            let current_byte: u64 = current_byte[0].into();
            value |= (current_byte & SEGMENT_BITS_U64) << shift;

            if (current_byte & CONTINUE_BIT_U64) == 0 {
                break;
            }

            shift += 7;
            if shift >= 64 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "VarInt too big",
                ));
            }
        }

        Ok(value as i64)
    }

    fn decode_str(&mut self) -> std::io::Result<String> {
        let length: usize = self.decode_vari32()?.try_into().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid string length")
        })?;
        let mut buffer = vec![0; length];
        self.read_exact(&mut buffer)?;
        Ok(String::from_utf8(buffer).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid UTF-8 string")
        })?)
    }
}

impl PacketDecode for &[u8] {}

/// A trait for decoding various data types from a byte stream asynchronously.
///
/// This trait extends the `AsyncReadExt` trait from the `tokio` crate and provides
/// a method for decoding vari32 from a byte stream asynchronously, according to
/// the Minecraft protocol.
#[async_trait]
pub trait AsyncPacketDecode: AsyncReadExt + Unpin {
    async fn decode_vari32(&mut self) -> std::io::Result<i32> {
        let mut value: u32 = 0;
        let mut shift: u8 = 0;
        let mut current_byte = [0u8; 1];

        loop {
            self.read_exact(&mut current_byte).await?;
            let current_byte: u32 = current_byte[0].into();
            value |= (current_byte & SEGMENT_BITS_U32) << shift;

            if (current_byte & CONTINUE_BIT_U32) == 0 {
                break;
            }

            shift += 7;
            if shift >= 32 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "VarInt too big",
                ));
            }
        }

        Ok(value as i32)
    }
}

#[async_trait]
impl AsyncPacketDecode for TcpStream {}

#[cfg(test)]
mod tests {
    use super::{AsyncPacketDecode, AsyncPacketEncode, PacketDecode, PacketEncode};
    use tokio::io::BufReader;

    impl AsyncPacketDecode for BufReader<&[u8]> {}
    impl AsyncPacketEncode for Vec<u8> {}

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
    fn test_encode_f32() {
        let mut buffer = Vec::new();
        buffer.encode_f32(3.14159).expect("Encoding failed");
        assert_eq!(buffer, vec![0x40, 0x49, 0x0f, 0xd0]);
    }

    fn test_decode_f32(buffer: &[u8]) {
        let mut buffer: &[u8] = &[0x40, 0x49, 0x0f, 0xd0];
        assert_eq!(buffer.decode_f32().expect("Decoding failed"), 3.14159);
    }

    fn test_encode_f64() {
        let mut buffer = Vec::new();
        buffer.encode_f64(3.14159).expect("Encoding failed");
        assert_eq!(buffer, vec![0x40, 0x09, 0x21, 0xf9, 0xf0, 0x1b, 0x86, 0x6e]);
    }

    fn test_decode_f64() {
        let mut buffer: &[u8] = &[0x40, 0x09, 0x21, 0xf9, 0xf0, 0x1b, 0x86, 0x6e];
        assert_eq!(buffer.decode_f64().expect("Decoding failed"), 3.14159);
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
        PacketEncode::encode_vari32(&mut buffer, 0x12345678).expect("Encoding failed");
        assert_eq!(buffer, vec![0xf8, 0xac, 0xd1, 0x91, 0x01]);
    }

    #[tokio::test]
    async fn test_async_encode_vari32() {
        let mut buffer = Vec::new();

        AsyncPacketEncode::encode_vari32(&mut buffer, 0x12345678)
            .await
            .expect("Encoding failed");

        assert_eq!(buffer, vec![0xf8, 0xac, 0xd1, 0x91, 0x01]);
    }

    #[test]
    fn test_decode_vari32() {
        let mut buffer: &[u8] = &[0xf8, 0xac, 0xd1, 0x91, 0x01];
        assert_eq!(buffer.decode_vari32().expect("Decoding failed"), 0x12345678);
    }

    #[tokio::test]
    async fn test_async_decode_vari32() {
        let data: &[u8] = &[0xf8, 0xac, 0xd1, 0x91, 0x01];
        let mut buffer = BufReader::new(data);
        assert_eq!(
            buffer.decode_vari32().await.expect("Decoding failed"),
            0x12345678
        );
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
    fn test_decode_vari64() {
        let mut buffer: &[u8] = &[0xef, 0x9b, 0xaf, 0x85, 0x89, 0xcf, 0x95, 0x9a, 0x12];
        assert_eq!(
            buffer.decode_vari64().expect("Decoding failed"),
            0x1234567890abcdef
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

    #[test]
    fn test_decode_str() {
        let mut buffer: &[u8] = &[
            0x0d, 0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x2c, 0x20, 0x57, 0x6f, 0x72, 0x6c, 0x64, 0x21,
        ];
        assert_eq!(
            buffer.decode_str().expect("Decoding failed"),
            "Hello, World!"
        );
    }
}
