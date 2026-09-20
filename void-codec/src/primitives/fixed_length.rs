use crate::{Decode, DecodeError, Decoder, Encode, LimitKind};

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
    decoder: &mut Decoder<'_>,
) -> Result<Vec<T>, DecodeError> {
    decoder.charge_elements(len)?;
    let allocation = len
        .checked_mul(std::mem::size_of::<T>())
        .ok_or(DecodeError::InvalidLength)?;
    decoder.charge_allocation(allocation)?;
    let mut vec = Vec::new();
    vec.try_reserve_exact(len)
        .map_err(|_| DecodeError::AllocationFailed {
            requested: allocation,
        })?;
    for _ in 0..len {
        vec.push(decoder.decode::<T>()?);
    }
    Ok(vec)
}

/// Optimized decoding for Vec<u8> - just copy_from_slice without decode overhead
pub fn decode_fixed_length_vec_u8(
    len: usize,
    decoder: &mut Decoder<'_>,
) -> Result<Vec<u8>, DecodeError> {
    decoder.charge_elements(len)?;
    decoder.charge_allocation(len)?;
    let data = decoder.take(len)?;
    Ok(data.to_vec())
}
/// Encode remaining bytes - for Vec<u8> at end of packet
pub fn encode_remaining_vec_u8(vec: &[u8], buf: &mut Vec<u8>) {
    buf.extend_from_slice(vec);
}

/// Decode remaining bytes - consumes all remaining buffer
pub fn decode_remaining_vec_u8(decoder: &mut Decoder<'_>) -> Result<Vec<u8>, DecodeError> {
    let len = decoder.remaining_len();
    if len > decoder.limits().max_remaining_bytes {
        return Err(DecodeError::LimitExceeded {
            kind: LimitKind::RemainingBytes,
            requested: len,
            limit: decoder.limits().max_remaining_bytes,
        });
    }
    decoder.charge_allocation(len)?;
    Ok(decoder.take(len)?.to_vec())
}
