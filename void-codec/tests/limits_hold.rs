use ussr_nbt::owned::Nbt;
use voidmc_codec::{DecodeError, DecodeLimits, Decoder, Encode, LimitKind, VarI32};

fn nested_compounds(levels: usize) -> Vec<u8> {
    let mut bytes = vec![0x0A];
    for _ in 0..levels {
        bytes.extend_from_slice(&[0x0A, 0x00, 0x00]);
    }
    bytes.extend(std::iter::repeat_n(0x00, levels + 1));
    bytes
}

fn kind(result: Result<impl std::fmt::Debug, DecodeError>) -> Option<LimitKind> {
    match result {
        Err(DecodeError::LimitExceeded { kind, .. }) => Some(kind),
        _ => None,
    }
}

#[test]
fn nbt_nested_past_its_depth_is_refused() {
    let limits = DecodeLimits {
        max_nbt_depth: 8,
        ..DecodeLimits::default()
    };
    let deep = nested_compounds(20);
    assert_eq!(
        kind(Decoder::new(&deep, limits).decode::<Nbt>()),
        Some(LimitKind::NbtDepth)
    );
    let shallow = nested_compounds(3);
    assert!(Decoder::new(&shallow, limits).decode::<Nbt>().is_ok());
}

#[test]
fn a_vec_announced_past_the_allocation_budget_is_refused_before_reading() {
    let limits = DecodeLimits {
        max_allocation_bytes: 1024,
        ..DecodeLimits::default()
    };
    let mut bytes = Vec::new();
    VarI32(1000).encode(&mut bytes);
    assert_eq!(
        kind(Decoder::new(&bytes, limits).decode::<Vec<u64>>()),
        Some(LimitKind::AllocationBytes)
    );
}

#[test]
fn a_string_announced_past_the_allocation_budget_is_refused_before_reading() {
    let limits = DecodeLimits {
        max_allocation_bytes: 16,
        ..DecodeLimits::default()
    };
    let mut bytes = Vec::new();
    VarI32(17).encode(&mut bytes);
    bytes.extend_from_slice(&[b'a'; 17]);
    assert_eq!(
        kind(Decoder::new(&bytes, limits).decode::<String>()),
        Some(LimitKind::AllocationBytes)
    );
}
