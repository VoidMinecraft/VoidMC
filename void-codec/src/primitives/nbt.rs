use std::fmt;

use crate::{Decode, DecodeError, Decoder, Encode, LimitKind};
use tracing::{error, warn};
use ussr_nbt::mutf8::MString;
use ussr_nbt::owned::{Compound, List, Nbt, Tag};
use ussr_nbt::{ReadOpts, WriteOpts};

/// The client reads an NBT string length as an unsigned 16-bit prefix.
pub const MAX_STRING_BYTES: usize = u16::MAX as usize;

/// What a character costs in an NBT string: Java's modified UTF-8 writes NUL on 2 bytes and a
/// character outside the BMP as a surrogate pair, 6 bytes.
pub const fn modified_utf8_width(c: char) -> usize {
    match c {
        '\0' => 2,
        c if c as u32 >= 0x1_0000 => 6,
        c => c.len_utf8(),
    }
}

/// The length in bytes of the longest prefix of `text`, cut on a character, that fits in an NBT
/// string.
pub fn fitting_len(text: &str) -> usize {
    let mut width = 0;
    for (index, c) in text.char_indices() {
        width += modified_utf8_width(c);
        if width > MAX_STRING_BYTES {
            return index;
        }
    }
    text.len()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NbtStringTooLong {
    pub bytes: usize,
}

impl fmt::Display for NbtStringTooLong {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "an NBT string takes {} modified UTF-8 bytes, the client reads at most {MAX_STRING_BYTES}",
            self.bytes
        )
    }
}

impl std::error::Error for NbtStringTooLong {}

/// Refuses an NBT that holds a string, name or value, longer than the client reads.
pub fn check(nbt: &Nbt) -> Result<(), NbtStringTooLong> {
    match longest_in(&nbt.compound) {
        bytes if bytes > MAX_STRING_BYTES => Err(NbtStringTooLong { bytes }),
        _ => Ok(()),
    }
}

fn longest_in(compound: &Compound) -> usize {
    compound
        .tags
        .iter()
        .map(|(name, tag)| name.len().max(longest_in_tag(tag)))
        .max()
        .unwrap_or(0)
}

fn longest_in_tag(tag: &Tag) -> usize {
    match tag {
        Tag::String(text) => text.len(),
        Tag::List(list) => longest_in_list(list),
        Tag::Compound(compound) => longest_in(compound),
        _ => 0,
    }
}

fn longest_in_list(list: &List) -> usize {
    match list {
        List::String(texts) => texts.iter().map(|text| text.len()).max().unwrap_or(0),
        List::List(lists) => lists.iter().map(longest_in_list).max().unwrap_or(0),
        List::Compound(compounds) => compounds.iter().map(longest_in).max().unwrap_or(0),
        _ => 0,
    }
}

fn fit_compound(compound: &mut Compound) {
    for (name, tag) in &mut compound.tags {
        fit_string(name);
        match tag {
            Tag::String(text) => fit_string(text),
            Tag::List(list) => fit_list(list),
            Tag::Compound(compound) => fit_compound(compound),
            _ => {}
        }
    }
}

fn fit_list(list: &mut List) {
    match list {
        List::String(texts) => texts.iter_mut().for_each(fit_string),
        List::List(lists) => lists.iter_mut().for_each(fit_list),
        List::Compound(compounds) => compounds.iter_mut().for_each(fit_compound),
        _ => {}
    }
}

fn fit_string(text: &mut MString) {
    if text.len() <= MAX_STRING_BYTES {
        return;
    }
    let Ok(decoded) = text.decode() else {
        error!(
            bytes = text.len(),
            "NBT string is not modified UTF-8 and cannot be cut on a character"
        );
        return;
    };
    *text = MString::from(&decoded[..fitting_len(&decoded)]);
}

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
        // Send the protocol header bytes: 0x0A (tag) + 0x00 0x00 (empty root name)
        let Some(first) = buf.first_mut() else {
            return Ok(0);
        };
        if self.read_bytes == 0 {
            *first = 0x0A; // Compound tag
            self.read_bytes = 1;
            return Ok(1);
        }
        if self.read_bytes == 1 {
            *first = 0x00; // Name length high byte
            self.read_bytes = 2;
            return Ok(1);
        }
        if self.read_bytes == 2 {
            *first = 0x00; // Name length low byte
            self.read_bytes = 3;
            // Also consume one dummy byte from inner since NBT expects to read the name
            let _ = self.inner.read(&mut [0])?;
            return Ok(1);
        }

        // After header, read the payload directly from inner
        match self.inner.read(buf) {
            Ok(n) => {
                self.read_bytes = self.read_bytes.saturating_add(n);
                Ok(n)
            }
            Err(e) => Err(e),
        }
    }
}

impl Encode for Nbt {
    fn encode(&self, buf: &mut Vec<u8>) {
        match check(self) {
            Ok(()) => write_network(self, buf),
            Err(too_long) => {
                warn!(
                    bytes = too_long.bytes,
                    "NBT string over the client limit, cut to fit"
                );
                let mut fitted = self.clone();
                fit_compound(&mut fitted.compound);
                write_network(&fitted, buf);
            }
        }
    }
}

fn write_network(nbt: &Nbt, buf: &mut Vec<u8>) {
    let start = buf.len();
    if let Err(failure) = nbt.write_with_opts(buf, WriteOpts { name: false }) {
        error!(%failure, "NBT could not be written: an empty compound goes instead");
        buf.truncate(start);
        buf.extend_from_slice(&[0x0A, 0x00]);
    }
}

impl Decode for Nbt {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let original = decoder.remaining();
        let start_len = original.len();
        let [root_tag] = nbt_take_array(decoder, start_len)?;
        if root_tag != 0x0A {
            return Err(DecodeError::InvalidLength);
        }
        scan_compound(decoder, start_len, 0)?;
        let consumed = start_len
            .checked_sub(decoder.remaining_len())
            .ok_or(DecodeError::InvalidLength)?;
        let nbt_bytes = original.get(..consumed).ok_or(DecodeError::InvalidLength)?;

        // Wrap the buffer with the NBT reader that prepends the header
        let mut reader = NbtReader::new(nbt_bytes);

        let opts = ReadOpts::new().with_depth_limit(decoder.limits().max_nbt_depth.ussr_limit());
        match Nbt::read_with_opts(&mut reader, opts) {
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
    nbt_charge(decoder, start_len, len)?;
    decoder.take(len)
}

fn nbt_charge(decoder: &Decoder<'_>, start_len: usize, len: usize) -> Result<(), DecodeError> {
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
    Ok(())
}

fn nbt_take_array<const N: usize>(
    decoder: &mut Decoder<'_>,
    start_len: usize,
) -> Result<[u8; N], DecodeError> {
    nbt_charge(decoder, start_len, N)?;
    decoder.take_array()
}

fn nbt_i32(decoder: &mut Decoder<'_>, start_len: usize) -> Result<i32, DecodeError> {
    Ok(i32::from_be_bytes(nbt_take_array(decoder, start_len)?))
}

fn scan_string(decoder: &mut Decoder<'_>, start_len: usize) -> Result<(), DecodeError> {
    let len = usize::from(u16::from_be_bytes(nbt_take_array(decoder, start_len)?));
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
        let [tag] = nbt_take_array(decoder, start_len)?;
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
        scan_payload(decoder, start_len, tag, depth)?;
    }
}

fn scan_payload(
    decoder: &mut Decoder<'_>,
    start_len: usize,
    tag: u8,
    depth: usize,
) -> Result<(), DecodeError> {
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
            let list_depth = deeper(depth)?;
            check_nbt_depth(decoder, list_depth)?;
            let [element_tag] = nbt_take_array(decoder, start_len)?;
            let raw_count = nbt_i32(decoder, start_len)?;
            let count = checked_nbt_count(decoder, raw_count)?;
            if element_tag == 0 && count != 0 {
                return Err(DecodeError::InvalidLength);
            }
            decoder.charge_elements(count)?;
            decoder.charge_allocation(count.checked_mul(32).ok_or(DecodeError::InvalidLength)?)?;
            for _ in 0..count {
                scan_payload(decoder, start_len, element_tag, list_depth)?;
            }
        }
        10 => scan_compound(decoder, start_len, deeper(depth)?)?,
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

fn deeper(depth: usize) -> Result<usize, DecodeError> {
    depth.checked_add(1).ok_or(DecodeError::InvalidLength)
}

fn check_nbt_depth(decoder: &Decoder<'_>, depth: usize) -> Result<(), DecodeError> {
    let limit = decoder.limits().max_nbt_depth.levels();
    if depth > limit {
        return Err(DecodeError::LimitExceeded {
            kind: LimitKind::NbtDepth,
            requested: depth,
            limit,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Nbt {
        Nbt {
            name: "".into(),
            compound: Compound {
                tags: vec![("text".into(), Tag::String(value.into()))],
            },
        }
    }

    fn sent_text(nbt: &Nbt) -> (usize, String) {
        let mut bytes = Vec::new();
        nbt.encode(&mut bytes);
        let len = u16::from_be_bytes([bytes[8], bytes[9]]) as usize;
        let decoded = Nbt::decode(&mut bytes.as_slice()).expect("the client reads it");
        let Tag::String(value) = &decoded.compound.tags[0].1 else {
            panic!("a string");
        };
        (
            len,
            value.decode().expect("valid modified UTF-8").into_owned(),
        )
    }

    #[test]
    fn check_refuses_a_string_the_client_cannot_read() {
        let emoji = format!("a{}", "\u{1F600}".repeat(11_000));
        assert_eq!(
            check(&text(&emoji)),
            Err(NbtStringTooLong { bytes: 66_001 })
        );
        assert_eq!(check(&text(&"a".repeat(MAX_STRING_BYTES))), Ok(()));
        let nested = Nbt {
            name: "".into(),
            compound: Compound {
                tags: vec![(
                    "lines".into(),
                    Tag::List(List::Compound(vec![Compound {
                        tags: vec![(
                            "messages".into(),
                            Tag::List(List::String(vec!["\0".repeat(40_000).into()])),
                        )],
                    }])),
                )],
            },
        };
        assert_eq!(check(&nested), Err(NbtStringTooLong { bytes: 80_000 }));
        let long_name = Nbt {
            name: "".into(),
            compound: Compound {
                tags: vec![("x".repeat(MAX_STRING_BYTES + 1).into(), Tag::Byte(1))],
            },
        };
        assert!(check(&long_name).is_err());
    }

    #[test]
    fn an_oversized_string_is_cut_on_a_character_and_still_decodes() {
        for value in [
            format!("a{}", "\u{1F600}".repeat(11_000)),
            "\u{1F600}".repeat(11_000),
            "a\0".repeat(30_000),
            "\u{e9}".repeat(40_000),
        ] {
            let (len, sent) = sent_text(&text(&value));
            assert!(len <= MAX_STRING_BYTES, "{len}");
            assert!(sent.len() < value.len());
            assert!(value.starts_with(&sent));
            assert_eq!(check(&text(&sent)), Ok(()));
        }
        let fits = "a".repeat(MAX_STRING_BYTES);
        assert_eq!(sent_text(&text(&fits)), (MAX_STRING_BYTES, fits));
    }

    #[test]
    fn a_cut_counts_nul_on_two_bytes() {
        let (len, sent) = sent_text(&text(&"\0".repeat(40_000)));
        assert_eq!(len, MAX_STRING_BYTES - 1);
        assert_eq!(sent.chars().count(), MAX_STRING_BYTES / 2);
    }

    #[test]
    fn a_cut_keeps_every_character_that_fits() {
        let value = format!("a{}", "\u{1F600}".repeat(11_000));
        let (len, sent) = sent_text(&text(&value));
        assert_eq!(sent.chars().count(), 1 + (MAX_STRING_BYTES - 1) / 6);
        assert_eq!(len, 1 + (MAX_STRING_BYTES - 1) / 6 * 6);
        let (_, sent) = sent_text(&text(&"\u{e9}".repeat(40_000)));
        assert_eq!(sent.chars().count(), MAX_STRING_BYTES / 2);
    }

    #[test]
    fn a_long_key_and_a_list_of_lists_are_cut_too() {
        let key = "\u{e9}".repeat(40_000);
        let nbt = Nbt {
            name: "".into(),
            compound: Compound {
                tags: vec![
                    (key.as_str().into(), Tag::Byte(1)),
                    (
                        "rows".into(),
                        Tag::List(List::List(vec![List::String(vec!["\u{1F600}"
                            .repeat(11_000)
                            .into()])])),
                    ),
                ],
            },
        };
        let mut bytes = Vec::new();
        nbt.encode(&mut bytes);
        let decoded = Nbt::decode(&mut bytes.as_slice()).expect("the client reads it");
        assert_eq!(check(&decoded), Ok(()));
        let name = decoded.compound.tags[0].0.decode().unwrap().into_owned();
        assert!(key.starts_with(&name));
        assert_eq!(name.chars().count(), MAX_STRING_BYTES / 2);
        let Tag::List(List::List(rows)) = &decoded.compound.tags[1].1 else {
            panic!("a list of lists");
        };
        let List::String(cells) = &rows[0] else {
            panic!("a list of strings");
        };
        assert_eq!(cells[0].len(), MAX_STRING_BYTES / 6 * 6);
    }

    #[test]
    fn a_long_string_only_in_a_list_of_lists_is_found_and_cut() {
        let nbt = Nbt {
            name: "".into(),
            compound: Compound {
                tags: vec![(
                    "rows".into(),
                    Tag::List(List::List(vec![List::String(vec![format!(
                        "a{}",
                        "\u{1F600}".repeat(11_000)
                    )
                    .into()])])),
                )],
            },
        };
        assert_eq!(check(&nbt), Err(NbtStringTooLong { bytes: 66_001 }));
        let mut bytes = Vec::new();
        nbt.encode(&mut bytes);
        let decoded = Nbt::decode(&mut bytes.as_slice()).expect("the client reads it");
        let Tag::List(List::List(rows)) = &decoded.compound.tags[0].1 else {
            panic!("a list of lists");
        };
        let List::String(cells) = &rows[0] else {
            panic!("a list of strings");
        };
        assert_eq!(cells[0].len(), 1 + (MAX_STRING_BYTES - 1) / 6 * 6);
    }

    #[test]
    fn writing_appends_to_what_the_buffer_already_holds() {
        let mut bytes = vec![0xFF, 0x01];
        text("hi").encode(&mut bytes);
        let mut alone = Vec::new();
        text("hi").encode(&mut alone);
        assert_eq!(bytes[..2], [0xFF, 0x01]);
        assert_eq!(bytes[2..], alone[..]);
    }

    #[test]
    fn fitting_len_stops_before_the_first_character_that_overflows() {
        assert_eq!(fitting_len("abc"), 3);
        let emoji = format!("a{}", "\u{1F600}".repeat(11_000));
        assert_eq!(fitting_len(&emoji), 1 + (MAX_STRING_BYTES - 1) / 6 * 4);
        assert_eq!(fitting_len(&"\0".repeat(40_000)), MAX_STRING_BYTES / 2);
    }

    #[test]
    fn a_named_root_still_goes_out_as_network_nbt() {
        let mut named = text("hi");
        named.name = "root".into();
        let mut bytes = Vec::new();
        named.encode(&mut bytes);
        let mut plain = Vec::new();
        text("hi").encode(&mut plain);
        assert_eq!(bytes, plain);
        let decoded = Nbt::decode(&mut bytes.as_slice()).expect("the client reads it");
        assert_eq!(decoded.compound, text("hi").compound);
    }

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
