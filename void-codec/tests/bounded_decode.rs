use proptest::prelude::*;
use ussr_nbt::owned::Nbt;
use voidmc_codec::{Decode, DecodeError, DecodeLimits, Decoder, LimitKind, VarI32};

struct ZeroWidth;

impl Decode for ZeroWidth {
    fn decode_with(_decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        Ok(Self)
    }
}

#[test]
fn huge_collection_count_is_rejected_before_reservation() {
    let bytes = [0xff, 0xff, 0xff, 0xff, 0x07];
    let limits = DecodeLimits {
        max_collection_elements: 16,
        ..DecodeLimits::default()
    };
    let mut decoder = Decoder::new(&bytes, limits);
    assert_eq!(
        decoder.decode::<Vec<u8>>(),
        Err(DecodeError::LimitExceeded {
            kind: LimitKind::CollectionElements,
            requested: i32::MAX as usize,
            limit: 16,
        })
    );
}

#[test]
fn zero_width_elements_are_bounded_by_work_budget() {
    let bytes = [10];
    let limits = DecodeLimits {
        max_collection_elements: 16,
        max_total_elements: 4,
        ..DecodeLimits::default()
    };
    let mut decoder = Decoder::new(&bytes, limits);
    assert!(matches!(
        decoder.decode::<Vec<ZeroWidth>>(),
        Err(DecodeError::LimitExceeded {
            kind: LimitKind::TotalElements,
            requested: 10,
            limit: 4,
        })
    ));
}

#[test]
fn string_limit_is_checked_before_copying() {
    let bytes = [5, b'h', b'e', b'l', b'l', b'o'];
    let limits = DecodeLimits {
        max_string_bytes: 4,
        ..DecodeLimits::default()
    };
    let mut decoder = Decoder::new(&bytes, limits);
    assert_eq!(
        decoder.decode::<String>(),
        Err(DecodeError::LimitExceeded {
            kind: LimitKind::StringBytes,
            requested: 5,
            limit: 4,
        })
    );
}

#[test]
fn negative_derived_fixed_length_is_rejected() {
    #[allow(dead_code)]
    #[derive(voidmc_codec::Decode)]
    struct Fixed {
        len: i32,
        #[codec(fixed_length = len)]
        values: Vec<u8>,
    }

    let bytes = (-1i32).to_be_bytes();
    let mut slice = bytes.as_slice();
    assert!(matches!(
        Fixed::decode(&mut slice),
        Err(DecodeError::InvalidLength)
    ));
}

#[test]
fn overflowing_derived_fixed_length_is_rejected() {
    #[allow(dead_code)]
    #[derive(voidmc_codec::Decode)]
    struct Fixed {
        len: u64,
        #[codec(fixed_length = len * 2)]
        values: Vec<u8>,
    }

    let bytes = u64::MAX.to_be_bytes();
    let mut slice = bytes.as_slice();
    assert!(matches!(
        Fixed::decode(&mut slice),
        Err(DecodeError::InvalidLength)
    ));
}

#[test]
fn malicious_nbt_array_length_is_rejected_before_library_parse() {
    let bytes = [
        0x0a, // root compound
        0x07, // byte-array entry
        0x00, 0x00, // empty entry name
        0x7f, 0xff, 0xff, 0xff, // i32::MAX elements
        0x00, // end compound
    ];
    let mut slice = bytes.as_slice();
    assert!(matches!(
        Nbt::decode(&mut slice),
        Err(DecodeError::LimitExceeded {
            kind: LimitKind::CollectionElements,
            ..
        })
    ));
}

#[test]
fn invalid_fifth_varint_byte_is_rejected() {
    let mut slice = [0xff, 0xff, 0xff, 0xff, 0x10].as_slice();
    assert_eq!(
        VarI32::decode(&mut slice),
        Err(DecodeError::InvalidVarintLength)
    );
}

proptest! {
    #[test]
    fn arbitrary_collection_and_nbt_inputs_are_bounded(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
        let limits = DecodeLimits {
            max_string_bytes: 32,
            max_collection_elements: 32,
            max_total_elements: 64,
            max_allocation_bytes: 512,
            max_remaining_bytes: 256,
            max_nbt_bytes: 256,
            max_nbt_depth: 8,
            max_decode_depth: 16,
        };

        let mut collection_decoder = Decoder::new(&bytes, limits);
        let _ = collection_decoder.decode::<Vec<Vec<u8>>>();

        let mut nbt_decoder = Decoder::new(&bytes, limits);
        let _ = nbt_decoder.decode::<Nbt>();
    }
}
