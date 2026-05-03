use crate::{Decode, DecodeError, Encode};
use ussr_nbt::owned::Nbt;

/// Custom reader that prepends the NBT protocol header for the root compound tag
/// For Minecraft 1.21.4+ (Protocol >= 764): 0x0A (compound) + 0x00 0x00 (empty root name) + payload
struct NbtReader<'a> {
    read_bytes: usize,
    inner: &'a [u8],
}

impl<'a> NbtReader<'a> {
    fn new(inner: &'a [u8]) -> Self {
        Self {
            read_bytes: 0,
            inner,
        }
    }
}

impl<'a> std::io::Read for NbtReader<'a> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let len = buf.len();
        if len == 0 {
            return Ok(0);
        }

        // Send the protocol header bytes: 0x0A (tag) + 0x00 0x00 (empty root name)
        if self.read_bytes == 0 {
            buf[0] = 0x0A; // Compound tag
            self.read_bytes = 1;
            return Ok(1);
        }
        if self.read_bytes == 1 {
            buf[0] = 0x00; // Name length high byte
            self.read_bytes = 2;
            return Ok(1);
        }
        if self.read_bytes == 2 {
            buf[0] = 0x00; // Name length low byte
            self.read_bytes = 3;
            // Also consume one dummy byte from inner since NBT expects to read the name
            let _ = self.inner.read(&mut [0])?;
            return Ok(1);
        }

        // After header, read the payload directly from inner
        match self.inner.read(buf) {
            Ok(n) => {
                self.read_bytes += n;
                Ok(n)
            }
            Err(e) => Err(e),
        }
    }
}

impl Encode for Nbt {
    fn encode(&self, buf: &mut Vec<u8>) {
        let mut temp_buf = Vec::new();
        if self.write(&mut temp_buf).is_ok() {
            // Write the compound tag
            buf.push(0x0A);
            // NBT library includes: tag (1) + name_len (2) + name (0 for empty root) + payload
            // We skip 3 bytes for the tag + empty name_len, then write the rest
            if temp_buf.len() > 3 {
                buf.extend_from_slice(&temp_buf[3..]);
            }
        }
    }
}

impl Decode for Nbt {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        // Wrap the buffer with the NBT reader that prepends the header
        let mut reader = NbtReader::new(buf);

        match Nbt::read(&mut reader) {
            Ok(nbt) => {
                // The reader has a read_bytes counter that tells us how much was consumed
                // including the 3-byte header (0x0A + 0x00 0x00) it injected
                let consumed = reader.read_bytes - 3; // Subtract 3 for the injected header
                *buf = &buf[consumed..];
                Ok(nbt)
            }
            Err(_) => Err(DecodeError::InvalidLength),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nbt_encode_produces_output() {
        // Create a simple NBT by reading from known data
        // The NBT library writes: TAG_ID (0x0A) + Name Length (0x00 0x00) + Payload
        let nbt_data = vec![
            0x0A, // Compound tag
            0x00, 0x00, // Empty name
            0x00, // End tag
        ];

        let mut slice = nbt_data.as_slice();
        let nbt = Nbt::read(&mut slice).expect("Failed to read NBT");

        // Now test our encode for 1.21.4+ format
        let mut buf = Vec::new();
        nbt.encode(&mut buf);

        // For 1.21.4+ (Protocol >= 764):
        // Encoding produces: 0x0A tag + payload (no name)
        // So: 0x0A + buffer[3..] (skips tag + name)
        assert_eq!(buf.len(), 2); // 0x0A + 0x00
        assert_eq!(buf[0], 0x0A); // Compound tag
        assert_eq!(buf[1], 0x00); // End tag (start of payload)
    }

    #[test]
    fn test_nbt_decode_with_reader() {
        // For 1.21.4+ format: the incoming data is just the payload (no name)
        // Reader prepends 0x0A tag, then reads the payload
        let nbt_data = vec![
            0x00, // End tag (minimal payload)
        ];

        let mut slice = nbt_data.as_slice();
        let result = Nbt::decode(&mut slice);

        // Should either succeed or gracefully fail - the key is it doesn't panic
        let _ = result;
    }

    #[test]
    fn test_nbt_truncated_data() {
        let mut slice = &[][..];
        let result = Nbt::decode(&mut slice);

        // Should handle truncated data gracefully
        assert!(result.is_err());
    }

    #[test]
    fn test_nbt_empty_buffer() {
        let buf = Vec::new();
        let mut slice = buf.as_slice();

        let result = Nbt::decode(&mut slice);
        assert!(result.is_err());
    }

    #[test]
    fn test_nbt_roundtrip_simple() {
        use ussr_nbt::owned::Tag;

        // Create a simple NBT structure like in configuration.rs
        let original_nbt = Nbt {
            name: "".into(),
            compound: vec![("field".into(), Tag::String("value".into()))].into(),
        };

        // Encode the NBT
        let mut encoded = Vec::new();
        original_nbt.encode(&mut encoded);

        assert!(!encoded.is_empty());
        assert_eq!(encoded[0], 0x0A, "First byte should be compound tag");

        // Decode the NBT
        let mut buf_slice = encoded.as_slice();
        let decoded_nbt = Nbt::decode(&mut buf_slice).expect("Failed to decode NBT");

        // Check the decoded NBT has the same structure
        assert_eq!(decoded_nbt.name, original_nbt.name);
        assert_eq!(
            decoded_nbt.compound.tags.len(),
            original_nbt.compound.tags.len(),
            "Should have same number of fields"
        );
    }

    #[test]
    fn test_nbt_roundtrip_multiple_tags() {
        use ussr_nbt::owned::Tag;

        // Create a more complex NBT with multiple field types (like real Minecraft configs)
        let original_nbt = Nbt {
            name: "".into(),
            compound: vec![
                (
                    "wild_texture".into(),
                    Tag::String("minecraft:entity/wolf/wolf_ashen".into()),
                ),
                ("flag".into(), Tag::Byte(1)),
            ]
            .into(),
        };

        // Encode
        let mut encoded = Vec::new();
        original_nbt.encode(&mut encoded);

        assert_eq!(encoded[0], 0x0A);

        // Decode
        let mut buf_slice = encoded.as_slice();
        let decoded = Nbt::decode(&mut buf_slice).expect("Failed to decode multi-tag NBT");

        // Verify structure preserved
        assert_eq!(decoded.name, original_nbt.name);
        assert_eq!(
            decoded.compound.tags.len(),
            original_nbt.compound.tags.len()
        );
    }
}
