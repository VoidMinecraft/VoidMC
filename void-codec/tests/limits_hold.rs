use ussr_nbt::owned::Nbt;
use voidmc_codec::{DecodeError, DecodeLimits, Decoder, Encode, LimitKind, NbtDepth, VarI32};

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

fn nested_lists(levels: usize) -> Vec<u8> {
    let mut bytes = vec![0x0A, 0x09, 0x00, 0x00];
    for _ in 1..levels {
        bytes.extend_from_slice(&[0x09, 0, 0, 0, 1]);
    }
    bytes.extend_from_slice(&[0x00, 0, 0, 0, 0, 0x00]);
    bytes
}

fn nbt_depth(bytes: &[u8], limit: usize) -> Result<(), DecodeError> {
    let limits = DecodeLimits {
        max_nbt_depth: NbtDepth::new(u16::try_from(limit).unwrap()).unwrap(),
        ..DecodeLimits::default()
    };
    Decoder::new(bytes, limits)
        .decode_exact::<Nbt>()
        .map(|_| ())
}

#[test]
fn compounds_nested_up_to_the_nbt_depth_are_accepted_and_one_more_is_refused() {
    assert_eq!(nbt_depth(&nested_compounds(8), 8), Ok(()));
    assert_eq!(
        nbt_depth(&nested_compounds(9), 8),
        Err(DecodeError::LimitExceeded {
            kind: LimitKind::NbtDepth,
            requested: 9,
            limit: 8,
        })
    );
}

#[test]
fn lists_nested_up_to_the_nbt_depth_are_accepted_and_one_more_is_refused() {
    assert_eq!(nbt_depth(&nested_lists(8), 8), Ok(()));
    assert_eq!(
        nbt_depth(&nested_lists(9), 8),
        Err(DecodeError::LimitExceeded {
            kind: LimitKind::NbtDepth,
            requested: 9,
            limit: 8,
        })
    );
}

fn payload(kinds: &[u8], bytes: &mut Vec<u8>) {
    match kinds.split_first() {
        Some((0x0A, rest)) => {
            match rest.first() {
                Some(&next) => bytes.extend_from_slice(&[next, 0, 0]),
                None => bytes.extend_from_slice(&[0x01, 0, 0]),
            }
            if rest.is_empty() {
                bytes.push(7);
            } else {
                payload(rest, bytes);
            }
            bytes.push(0x00);
        }
        Some((_, rest)) => match rest.first() {
            Some(&next) => {
                bytes.extend_from_slice(&[next, 0, 0, 0, 1]);
                payload(rest, bytes);
            }
            None => bytes.extend_from_slice(&[0x01, 0, 0, 0, 1, 7]),
        },
        None => {}
    }
}

fn nested(kinds: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x0A];
    let mut inner = vec![0x0A];
    inner.extend_from_slice(kinds);
    payload(&inner, &mut bytes);
    bytes
}

#[test]
fn mixed_nesting_counts_one_level_per_compound_or_list() {
    let mixed: Vec<u8> = (0..64)
        .map(|i| if i % 2 == 0 { 0x09 } else { 0x0A })
        .collect();
    let compounds = vec![0x0A; 64];
    for kinds in [mixed, compounds] {
        assert_eq!(nbt_depth(&nested(&kinds), 64), Ok(()));
        let mut deeper = kinds.clone();
        deeper.push(0x0A);
        assert_eq!(
            nbt_depth(&nested(&deeper), 64),
            Err(DecodeError::LimitExceeded {
                kind: LimitKind::NbtDepth,
                requested: 65,
                limit: 64,
            })
        );
    }
}

#[test]
fn the_default_nbt_depth_accepts_sixty_four_levels() {
    let limit = DecodeLimits::default().max_nbt_depth.levels();
    assert_eq!(limit, 64);
    assert_eq!(nbt_depth(&nested_compounds(limit), limit), Ok(()));
    assert_eq!(nbt_depth(&nested_lists(limit), limit), Ok(()));
    assert!(nbt_depth(&nested_compounds(limit + 1), limit).is_err());
}

#[test]
fn no_nbt_depth_above_512_can_be_built() {
    assert_eq!(NbtDepth::MAX.levels(), 512);
    assert_eq!(NbtDepth::new(512), Some(NbtDepth::MAX));
    assert_eq!(NbtDepth::new(513), None);
    assert_eq!(NbtDepth::new(u16::MAX), None);
}

#[test]
fn nbt_at_the_maximum_depth_fits_a_two_mebibyte_stack() {
    let handle = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let limit = NbtDepth::MAX.levels();
            let mixed: Vec<u8> = (0..limit)
                .map(|i| if i % 2 == 0 { 0x09 } else { 0x0A })
                .collect();
            let compounds = vec![0x0A; limit];
            for kinds in [mixed, compounds] {
                assert_eq!(nbt_depth(&nested(&kinds), limit), Ok(()));
                let mut deeper = kinds.clone();
                deeper.push(0x0A);
                assert_eq!(
                    nbt_depth(&nested(&deeper), limit),
                    Err(DecodeError::LimitExceeded {
                        kind: LimitKind::NbtDepth,
                        requested: 513,
                        limit: 512,
                    })
                );
            }
            assert_eq!(nbt_depth(&nested_lists(limit), limit), Ok(()));
            assert!(nbt_depth(&nested_lists(limit + 1), limit).is_err());
        })
        .unwrap();
    handle.join().unwrap();
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
