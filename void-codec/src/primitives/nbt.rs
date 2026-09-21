use crate::{Decode, DecodeError, Decoder, Encode, LimitKind};
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
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let original = decoder.remaining();
        let start_len = original.len();
        let root_tag = nbt_take(decoder, start_len, 1)?[0];
        if root_tag != 0x0A {
            return Err(DecodeError::InvalidLength);
        }
        scan_compound(decoder, start_len, 0)?;
        let consumed = start_len - decoder.remaining_len();
        let nbt_bytes = &original[..consumed];

        // Wrap the buffer with the NBT reader that prepends the header
        let mut reader = NbtReader::new(nbt_bytes);

        match Nbt::read(&mut reader) {
            Ok(nbt) => Ok(nbt),
            Err(_) => Err(DecodeError::InvalidLength),
        }
    }
}

fn nbt_take<'a>(
    decoder: &mut Decoder<'a>,
    start_len: usize,
    len: usize,
) -> Result<&'a [u8], DecodeError> {
    let consumed = start_len
        .checked_sub(decoder.remaining_len())
        .and_then(|value| value.checked_add(len))
        .ok_or(DecodeError::InvalidLength)?;
    if consumed > decoder.limits().max_nbt_bytes {
        return Err(DecodeError::LimitExceeded {
            kind: LimitKind::NbtBytes,
            requested: consumed,
            limit: decoder.limits().max_nbt_bytes,
        });
    }
    decoder.take(len)
}

fn nbt_i32(decoder: &mut Decoder<'_>, start_len: usize) -> Result<i32, DecodeError> {
    let bytes: [u8; 4] = nbt_take(decoder, start_len, 4)?
        .try_into()
        .expect("four bytes");
    Ok(i32::from_be_bytes(bytes))
}

fn scan_string(decoder: &mut Decoder<'_>, start_len: usize) -> Result<(), DecodeError> {
    let bytes: [u8; 2] = nbt_take(decoder, start_len, 2)?
        .try_into()
        .expect("two bytes");
    let len = usize::from(u16::from_be_bytes(bytes));
    decoder.charge_allocation(len)?;
    nbt_take(decoder, start_len, len)?;
    Ok(())
}

fn scan_compound(
    decoder: &mut Decoder<'_>,
    start_len: usize,
    depth: usize,
) -> Result<(), DecodeError> {
    check_nbt_depth(decoder, depth)?;
    let mut entries = 0usize;
    loop {
        let tag = nbt_take(decoder, start_len, 1)?[0];
        if tag == 0 {
            return Ok(());
        }
        entries = entries.checked_add(1).ok_or(DecodeError::InvalidLength)?;
        if entries > decoder.limits().max_collection_elements {
            return Err(DecodeError::LimitExceeded {
                kind: LimitKind::CollectionElements,
                requested: entries,
                limit: decoder.limits().max_collection_elements,
            });
        }
        decoder.charge_elements(1)?;
        decoder.charge_allocation(64)?;
        scan_string(decoder, start_len)?;
        scan_payload(decoder, start_len, tag, depth + 1)?;
    }
}

fn scan_payload(
    decoder: &mut Decoder<'_>,
    start_len: usize,
    tag: u8,
    depth: usize,
) -> Result<(), DecodeError> {
    check_nbt_depth(decoder, depth)?;
    match tag {
        1 => {
            nbt_take(decoder, start_len, 1)?;
        }
        2 => {
            nbt_take(decoder, start_len, 2)?;
        }
        3 | 5 => {
            nbt_take(decoder, start_len, 4)?;
        }
        4 | 6 => {
            nbt_take(decoder, start_len, 8)?;
        }
        7 => scan_array(decoder, start_len, 1)?,
        8 => scan_string(decoder, start_len)?,
        9 => {
            let element_tag = nbt_take(decoder, start_len, 1)?[0];
            let raw_count = nbt_i32(decoder, start_len)?;
            let count = checked_nbt_count(decoder, raw_count)?;
            if element_tag == 0 && count != 0 {
                return Err(DecodeError::InvalidLength);
            }
            decoder.charge_elements(count)?;
            decoder.charge_allocation(count.checked_mul(32).ok_or(DecodeError::InvalidLength)?)?;
            for _ in 0..count {
                scan_payload(decoder, start_len, element_tag, depth + 1)?;
            }
        }
        10 => scan_compound(decoder, start_len, depth + 1)?,
        11 => scan_array(decoder, start_len, 4)?,
        12 => scan_array(decoder, start_len, 8)?,
        _ => return Err(DecodeError::InvalidLength),
    }
    Ok(())
}

fn scan_array(
    decoder: &mut Decoder<'_>,
    start_len: usize,
    width: usize,
) -> Result<(), DecodeError> {
    let raw_count = nbt_i32(decoder, start_len)?;
    let count = checked_nbt_count(decoder, raw_count)?;
    decoder.charge_elements(count)?;
    let bytes = count.checked_mul(width).ok_or(DecodeError::InvalidLength)?;
    decoder.charge_allocation(bytes)?;
    nbt_take(decoder, start_len, bytes)?;
    Ok(())
}

fn checked_nbt_count(decoder: &Decoder<'_>, count: i32) -> Result<usize, DecodeError> {
    decoder.checked_len(
        count,
        LimitKind::CollectionElements,
        decoder.limits().max_collection_elements,
    )
}

fn check_nbt_depth(decoder: &Decoder<'_>, depth: usize) -> Result<(), DecodeError> {
    if depth > decoder.limits().max_nbt_depth {
        return Err(DecodeError::LimitExceeded {
            kind: LimitKind::NbtDepth,
            requested: depth,
            limit: decoder.limits().max_nbt_depth,
        });
    }
    Ok(())
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
