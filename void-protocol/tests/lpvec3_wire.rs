use voidmc_codec::{DecodeError, DecodeLimits, Encode};
use voidmc_protocol::types::LpVec3;

fn encoded(v: LpVec3) -> Vec<u8> {
    let mut bytes = Vec::new();
    v.encode(&mut bytes);
    bytes
}

#[test]
fn an_lpvec3_decodes_what_it_encodes() {
    let v = LpVec3 {
        x: 1.5,
        y: -0.25,
        z: 3.0,
    };
    let bytes = encoded(v);
    let back = DecodeLimits::default()
        .decode_exact::<LpVec3>(&bytes)
        .unwrap();
    assert!((back.x - v.x).abs() < 0.01);
    assert!((back.y - v.y).abs() < 0.01);
    assert!((back.z - v.z).abs() < 0.01);
    assert_eq!(
        DecodeLimits::default().decode_exact::<LpVec3>(&[0]),
        Ok(LpVec3::ZERO)
    );
}

#[test]
fn a_truncated_lpvec3_is_an_error_not_a_panic() {
    let bytes = encoded(LpVec3 {
        x: 40_000.0,
        y: 2.0,
        z: -7.0,
    });
    assert!(bytes.len() > 6);
    for end in 0..bytes.len() {
        let head = &bytes[..end];
        assert_eq!(
            DecodeLimits::default().with_decoder(head, |d| d.decode::<LpVec3>()),
            Err(DecodeError::UnexpectedEof),
            "cut at {end}"
        );
    }
}
