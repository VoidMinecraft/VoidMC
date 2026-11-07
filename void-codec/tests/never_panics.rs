use proptest::prelude::*;
use ussr_nbt::owned::Nbt;
use uuid::Uuid;
use voidmc_codec::{
    decode_fixed_length_vec, decode_fixed_length_vec_u8, decode_remaining_vec_u8, Decode,
    DecodeLimits, Decoder, Encode, NbtDepth, VarI32, VarI64,
};

fn tight() -> DecodeLimits {
    DecodeLimits {
        max_string_bytes: 64,
        max_collection_elements: 64,
        max_total_elements: 128,
        max_allocation_bytes: 4096,
        max_remaining_bytes: 512,
        max_nbt_bytes: 512,
        max_nbt_depth: NbtDepth::new(8).unwrap(),
        max_decode_depth: 16,
    }
}

fn every_primitive(bytes: &[u8], limits: DecodeLimits) {
    fn one<T: Decode>(bytes: &[u8], limits: DecodeLimits) {
        let _ = Decoder::new(bytes, limits).decode::<T>();
        let _ = Decoder::new(bytes, limits).decode_exact::<T>();
    }
    one::<bool>(bytes, limits);
    one::<u8>(bytes, limits);
    one::<i8>(bytes, limits);
    one::<u16>(bytes, limits);
    one::<i16>(bytes, limits);
    one::<i32>(bytes, limits);
    one::<u32>(bytes, limits);
    one::<i64>(bytes, limits);
    one::<u64>(bytes, limits);
    one::<f32>(bytes, limits);
    one::<f64>(bytes, limits);
    one::<VarI32>(bytes, limits);
    one::<VarI64>(bytes, limits);
    one::<Uuid>(bytes, limits);
    one::<String>(bytes, limits);
    one::<Option<String>>(bytes, limits);
    one::<Vec<u8>>(bytes, limits);
    one::<Vec<VarI32>>(bytes, limits);
    one::<Vec<Vec<String>>>(bytes, limits);
    one::<Vec<Option<Uuid>>>(bytes, limits);
    one::<Nbt>(bytes, limits);
    let _ = decode_fixed_length_vec::<VarI64>(7, &mut Decoder::new(bytes, limits));
    let _ = decode_fixed_length_vec_u8(33, &mut Decoder::new(bytes, limits));
    let _ = decode_remaining_vec_u8(&mut Decoder::new(bytes, limits));
}

fn with_length(length: i32, rest: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    VarI32(length).encode(&mut bytes);
    bytes.extend_from_slice(rest);
    bytes
}

fn nbt_lying(tag: u8, name: u16, count: i32, rest: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x0A, tag];
    bytes.extend_from_slice(&name.to_be_bytes());
    bytes.push(b'x');
    bytes.push(tag);
    bytes.extend_from_slice(&count.to_be_bytes());
    bytes.extend_from_slice(rest);
    bytes
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    #[test]
    fn decoding_random_bytes_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..96)) {
        every_primitive(&bytes, tight());
        every_primitive(&bytes, DecodeLimits::default());
    }

    #[test]
    fn a_lying_length_never_panics(
        length in prop_oneof![any::<i32>(), -2..300i32, Just(i32::MAX), Just(i32::MIN)],
        rest in prop::collection::vec(any::<u8>(), 0..48),
    ) {
        let bytes = with_length(length, &rest);
        every_primitive(&bytes, tight());
        every_primitive(&bytes, DecodeLimits::default());
    }

    #[test]
    fn a_lying_nbt_length_never_panics(
        tag in 0u8..16,
        name in any::<u16>(),
        count in prop_oneof![any::<i32>(), -2..64i32],
        rest in prop::collection::vec(any::<u8>(), 0..64),
    ) {
        let bytes = nbt_lying(tag, name, count, &rest);
        let _ = Decoder::new(&bytes, tight()).decode::<Nbt>();
        let _ = Decoder::new(&bytes, DecodeLimits::default()).decode::<Nbt>();
    }

    #[test]
    fn a_truncated_valid_value_never_panics(
        text in "\\PC{0,24}",
        numbers in prop::collection::vec(any::<i64>(), 0..8),
        cut in any::<prop::sample::Index>(),
    ) {
        let mut bytes = Vec::new();
        text.encode(&mut bytes);
        numbers.iter().map(|n| VarI64(*n)).collect::<Vec<_>>().encode(&mut bytes);
        let end = cut.index(bytes.len() + 1);
        let head = bytes.get(..end).unwrap_or_default();
        every_primitive(head, tight());
        every_primitive(head, DecodeLimits::default());
    }
}

#[test]
fn valid_values_decode_as_before() {
    let uuid = Uuid::from_u128(0x0123_4567_89ab_cdef_0011_2233_4455_6677);
    let mut bytes = Vec::new();
    true.encode(&mut bytes);
    200u8.encode(&mut bytes);
    (-3i8).encode(&mut bytes);
    VarI32(-1).encode(&mut bytes);
    VarI64(i64::MIN).encode(&mut bytes);
    uuid.encode(&mut bytes);
    let mut decoder = Decoder::new(&bytes, DecodeLimits::default());
    assert_eq!(decoder.decode::<bool>(), Ok(true));
    assert_eq!(decoder.decode::<u8>(), Ok(200));
    assert_eq!(decoder.decode::<i8>(), Ok(-3));
    assert_eq!(decoder.decode::<VarI32>(), Ok(VarI32(-1)));
    assert_eq!(decoder.decode::<VarI64>(), Ok(VarI64(i64::MIN)));
    assert_eq!(decoder.decode::<Uuid>(), Ok(uuid));
    assert_eq!(decoder.remaining_len(), 0);
}

#[test]
fn the_new_takes_stop_at_the_end() {
    let mut decoder = Decoder::new(&[7, 8, 9], DecodeLimits::default());
    assert_eq!(decoder.take_u8(), Ok(7));
    assert_eq!(decoder.take_array::<2>(), Ok([8, 9]));
    assert!(decoder.take_u8().is_err());
    assert!(decoder.take_array::<1>().is_err());
    assert_eq!(decoder.take_array::<0>(), Ok([]));
}
