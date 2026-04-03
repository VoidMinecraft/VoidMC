use crate::{Decode, DecodeError, Encode};

/// Helper functions for encoding/decoding fixed-length vectors
/// These functions do not encode a length prefix since it's known from context

pub fn encode_fixed_length_vec<T: Encode>(
    vec: &[T],
    expected_len: usize,
    buf: &mut Vec<u8>,
) -> Result<(), String> {
    if vec.len() != expected_len {
        return Err(format!(
            "Fixed-length vector length mismatch: expected {}, got {}",
            expected_len,
            vec.len()
        ));
    }
    for item in vec {
        item.encode(buf);
    }
    Ok(())
}

/// Optimized encoding for Vec<u8> - just extends the buffer directly
pub fn encode_fixed_length_vec_u8(
    vec: &[u8],
    expected_len: usize,
    buf: &mut Vec<u8>,
) -> Result<(), String> {
    if vec.len() != expected_len {
        return Err(format!(
            "Fixed-length vector length mismatch: expected {}, got {}",
            expected_len,
            vec.len()
        ));
    }
    buf.extend_from_slice(vec);
    Ok(())
}

pub fn decode_fixed_length_vec<T: Decode>(
    len: usize,
    buf: &mut &[u8],
) -> Result<Vec<T>, DecodeError> {
    let mut vec = Vec::with_capacity(len);
    for _ in 0..len {
        vec.push(T::decode(buf)?);
    }
    Ok(vec)
}

/// Optimized decoding for Vec<u8> - just copy_from_slice without decode overhead
pub fn decode_fixed_length_vec_u8(len: usize, buf: &mut &[u8]) -> Result<Vec<u8>, DecodeError> {
    if buf.len() < len {
        eprintln!("Buffer too small: expected {}, got {}", len, buf.len());
        return Err(DecodeError::UnexpectedEof);
    }
    let (data, rest) = buf.split_at(len);
    *buf = rest;
    Ok(data.to_vec())
}
/// Encode remaining bytes - for Vec<u8> at end of packet
pub fn encode_remaining_vec_u8(vec: &[u8], buf: &mut Vec<u8>) {
    buf.extend_from_slice(vec);
}

/// Decode remaining bytes - consumes all remaining buffer
pub fn decode_remaining_vec_u8(buf: &mut &[u8]) -> Result<Vec<u8>, DecodeError> {
    let remaining = buf.to_vec();
    *buf = &[];
    Ok(remaining)
}
