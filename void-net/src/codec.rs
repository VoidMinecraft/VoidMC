use async_trait::async_trait;
use std::io::{Read, Write};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use ussr_nbt::owned::*;
use uuid::Uuid;

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

    fn encode_nbt(&mut self, value: &Nbt) -> std::io::Result<()> {
        let mut buffer = Vec::new();
        value.write(&mut buffer)?;
        self.write_all(&[0x0A])?;
        if buffer.len() > 3 {
            self.write_all(&buffer[3..])
        } else {
            Ok(())
        }
    }

    fn encode_uuid(&mut self, value: Uuid) -> std::io::Result<()> {
        self.write_all(value.as_bytes())
    }
}

impl<T: Write> PacketEncode for T {}

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

pub struct CustomReader<'a, T: Read> {
    read_bytes: usize,
    inner: &'a mut T,
}

impl<'a, T: Read> Read for CustomReader<'a, T> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let len = buf.len();
        if len == 0 {
            return Ok(0);
        }
        if self.read_bytes == 0 {
            buf[0] = 0x0A;
            self.read_bytes = 1;
            return Ok(1);
        }
        if self.read_bytes == 1 {
            buf[0] = 0x00;
            self.read_bytes = 2;
            return Ok(1);
        }
        if self.read_bytes == 2 {
            buf[0] = 0x00;
            self.read_bytes = 3;
            self.inner.read(&mut [0])?;
            return Ok(1);
        } //TODO: make this better
        self.inner.read(buf)
    }
}

#[async_trait]
impl<T: AsyncWriteExt + Unpin> AsyncPacketEncode for T {}

/// A trait for decoding various data types from a byte stream.
///
/// This trait extends the `Read` trait and provides methods for decoding
/// different primitive types and strings from a byte stream, according
pub trait PacketDecode: Read + Sized {
    /// to the Minecraft protocol.
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

    fn decode_nbt(&mut self) -> std::io::Result<Nbt> {
        match Nbt::read(&mut CustomReader {
            read_bytes: 0,
            inner: self,
        }) {
            Ok(value) => Ok(value),
            Err(_) => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid NBT data",
            )),
        }
    }

    fn decode_uuid(&mut self) -> std::io::Result<Uuid> {
        let mut buffer = [0; 16];
        self.read_exact(&mut buffer)?;
        Ok(Uuid::from_bytes(buffer))
    }
}
impl<T: Read + Sized> PacketDecode for T {}

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
impl<T: AsyncReadExt + Unpin> AsyncPacketDecode for T {}

#[cfg(test)]
mod tests {
    use super::{AsyncPacketDecode, AsyncPacketEncode, PacketDecode, PacketEncode};
    use tokio::io::BufReader;
    use ussr_nbt::owned::{List, Nbt, Tag};
    use uuid::Uuid;

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

    #[test]
    fn test_decode_f32() {
        let mut buffer: &[u8] = &[0x40, 0x49, 0x0f, 0xd0];
        assert_eq!(buffer.decode_f32().expect("Decoding failed"), 3.14159);
    }

    #[test]
    fn test_encode_f64() {
        let mut buffer = Vec::new();
        buffer.encode_f64(3.14159).expect("Encoding failed");
        assert_eq!(buffer, vec![0x40, 0x09, 0x21, 0xf9, 0xf0, 0x1b, 0x86, 0x6e]);
    }

    #[test]
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
        assert_eq!(
            PacketDecode::decode_vari32(&mut buffer).expect("Decoding failed"),
            0x12345678
        );
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

    #[test]
    fn test_decode_nbt() {
        let mut buf: &[u8] = &[
            10, 1, 0, 9, 84, 101, 115, 116, 32, 98, 121, 116, 101, 123, 8, 0, 11, 84, 101, 115,
            116, 32, 115, 116, 114, 105, 110, 103, 0, 11, 72, 101, 108, 108, 111, 44, 32, 78, 66,
            84, 33, 9, 0, 9, 84, 101, 115, 116, 32, 108, 105, 115, 116, 5, 0, 0, 0, 3, 63, 128, 0,
            0, 64, 0, 0, 0, 64, 64, 0, 0, 0, 0, 0, //added bytes to test read
        ];

        let nbt = buf.decode_nbt().expect("Decoding failed");

        assert_eq!(nbt.name.to_string(), "");
        assert_eq!(nbt.compound.tags.len(), 3);
        assert_eq!(nbt.compound.tags[0].0.to_string(), "Test byte");
        assert_eq!(nbt.compound.tags[0].1, Tag::Byte(123));
        assert_eq!(nbt.compound.tags[1].0.to_string(), "Test string");
        assert_eq!(nbt.compound.tags[1].1, Tag::String("Hello, NBT!".into()));
        assert_eq!(nbt.compound.tags[2].0.to_string(), "Test list");
        if let Tag::List(List::Float(items)) = &nbt.compound.tags[2].1 {
            let items = items.to_vec();
            assert_eq!(items.len(), 3);
            assert_eq!(items[0], 1.0);
            assert_eq!(items[1], 2.0);
            assert_eq!(items[2], 3.0);
        } else {
            assert!(false);
        }
        assert_eq!(buf.len(), 2);
    }

    #[test]
    fn test_encode_nbt() {
        let mut buffer = vec![];

        buffer
            .encode_nbt(&Nbt {
                name: "".into(),
                compound: vec![
                    ("Test byte".into(), 123u8.into()),
                    ("Test string".into(), "Hello, NBT!".into()),
                    ("Test list".into(), vec![1f32, 2f32, 3f32].into()),
                ]
                .into(),
            })
            .expect("Encoding failed");

        assert_eq!(
            buffer,
            &[
                10, 1, 0, 9, 84, 101, 115, 116, 32, 98, 121, 116, 101, 123, 8, 0, 11, 84, 101, 115,
                116, 32, 115, 116, 114, 105, 110, 103, 0, 11, 72, 101, 108, 108, 111, 44, 32, 78,
                66, 84, 33, 9, 0, 9, 84, 101, 115, 116, 32, 108, 105, 115, 116, 5, 0, 0, 0, 3, 63,
                128, 0, 0, 64, 0, 0, 0, 64, 64, 0, 0, 0
            ]
        );
    }

    #[test]
    fn test_encode_uuid() {
        let mut buffer = Vec::new();
        buffer
            .encode_uuid(Uuid::parse_str("7fd2fd2c-b6d7-4ddf-b642-6c329296adf8").unwrap())
            .expect("Encoding failed");
        assert_eq!(
            buffer,
            vec![
                0x7f, 0xd2, 0xfd, 0x2c, 0xb6, 0xd7, 0x4d, 0xdf, 0xb6, 0x42, 0x6c, 0x32, 0x92, 0x96,
                0xad, 0xf8
            ]
        );
    }

    #[test]
    fn test_decode_uuid() {
        let mut buffer: &[u8] = &[
            0x7f, 0xd2, 0xfd, 0x2c, 0xb6, 0xd7, 0x4d, 0xdf, 0xb6, 0x42, 0x6c, 0x32, 0x92, 0x96,
            0xad, 0xf8,
        ];
        assert_eq!(
            buffer.decode_uuid().expect("Decoding failed"),
            Uuid::parse_str("7fd2fd2c-b6d7-4ddf-b642-6c329296adf8").unwrap()
        );
    }
}
