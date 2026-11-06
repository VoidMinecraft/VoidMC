//! Frames as they travel: a length, then a packet, compressed once a backend asks for it.
//!
//! A frame here is the packet id followed by its body, already inflated. The rest of the
//! crate never sees compression.

use std::fmt;
use std::io::{Read, Write};

use bytes::{Buf, Bytes, BytesMut};
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use voidmc_codec::{Decode, Encode, VarI32};

pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum WireError {
    BadLength,
    TooLarge(usize),
    BadCompression,
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadLength => write!(f, "invalid frame length"),
            Self::TooLarge(len) => write!(f, "frame of {len} bytes is above the limit"),
            Self::BadCompression => write!(f, "invalid compressed frame"),
        }
    }
}

impl std::error::Error for WireError {}

/// How one side of the proxy frames its packets.
#[derive(Debug, Default, Clone, Copy)]
pub struct Framing {
    threshold: Option<usize>,
}

impl Framing {
    /// A negative threshold turns compression off, as `login_compression` defines it.
    pub fn set_threshold(&mut self, threshold: i32) {
        self.threshold = usize::try_from(threshold).ok();
    }

    pub fn compressed(&self) -> bool {
        self.threshold.is_some()
    }

    /// The next whole frame of `buffer`, or `None` until more bytes arrive.
    pub fn decode(&self, buffer: &mut BytesMut) -> Result<Option<Bytes>, WireError> {
        let Some((len, prefix)) = read_length(buffer)? else {
            return Ok(None);
        };
        if len > MAX_FRAME_BYTES {
            return Err(WireError::TooLarge(len));
        }
        if buffer.len() < prefix + len {
            buffer.reserve(prefix + len - buffer.len());
            return Ok(None);
        }
        buffer.advance(prefix);
        let frame = buffer.split_to(len).freeze();
        if frame.is_empty() {
            return Err(WireError::BadLength);
        }
        match self.threshold {
            Some(_) => inflate(frame).map(Some),
            None => Ok(Some(frame)),
        }
    }

    pub fn encode(&self, frame: &[u8], out: &mut Vec<u8>) {
        let Some(threshold) = self.threshold else {
            VarI32(frame.len() as i32).encode(out);
            out.extend_from_slice(frame);
            return;
        };
        let mut body = Vec::with_capacity(frame.len() / 2 + 8);
        if frame.len() < threshold {
            VarI32(0).encode(&mut body);
            body.extend_from_slice(frame);
        } else {
            VarI32(frame.len() as i32).encode(&mut body);
            let mut encoder = ZlibEncoder::new(body, Compression::fast());
            encoder.write_all(frame).expect("a Vec takes every write");
            body = encoder.finish().expect("a Vec takes every write");
        }
        VarI32(body.len() as i32).encode(out);
        out.extend_from_slice(&body);
    }
}

fn read_length(buffer: &[u8]) -> Result<Option<(usize, usize)>, WireError> {
    let mut value = 0u32;
    for (position, byte) in buffer.iter().take(5).enumerate() {
        value |= u32::from(byte & 0x7F) << (7 * position);
        if byte & 0x80 == 0 {
            let len = i32::try_from(value).map_err(|_| WireError::BadLength)?;
            return Ok(Some((len as usize, position + 1)));
        }
    }
    if buffer.len() >= 5 {
        return Err(WireError::BadLength);
    }
    Ok(None)
}

fn inflate(frame: Bytes) -> Result<Bytes, WireError> {
    let mut rest: &[u8] = &frame;
    let data_length = VarI32::decode(&mut rest)
        .map_err(|_| WireError::BadCompression)?
        .0;
    let consumed = frame.len() - rest.len();
    if data_length == 0 {
        return Ok(frame.slice(consumed..));
    }
    let expected = usize::try_from(data_length).map_err(|_| WireError::BadCompression)?;
    if expected > MAX_FRAME_BYTES {
        return Err(WireError::TooLarge(expected));
    }
    let mut inflated = Vec::with_capacity(expected);
    ZlibDecoder::new(rest)
        .take(expected as u64 + 1)
        .read_to_end(&mut inflated)
        .map_err(|_| WireError::BadCompression)?;
    if inflated.len() != expected {
        return Err(WireError::BadCompression);
    }
    Ok(inflated.into())
}

/// The packet id of a frame, and what follows it.
pub fn split(frame: &[u8]) -> Option<(i32, &[u8])> {
    let mut body = frame;
    let id = VarI32::decode(&mut body).ok()?.0;
    Some((id, body))
}

/// A frame whose body is written by hand, for the packets `voidmc-protocol` has no type for.
pub fn frame(id: i32, write: impl FnOnce(&mut Vec<u8>)) -> Bytes {
    let mut buffer = Vec::new();
    VarI32(id).encode(&mut buffer);
    write(&mut buffer);
    buffer.into()
}

/// A frame out of a tagged packet of `voidmc-protocol`, which writes its own id.
pub fn tagged(packet: &impl Encode) -> Bytes {
    let mut buffer = Vec::new();
    packet.encode(&mut buffer);
    buffer.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrapped(framing: Framing, payload: &[u8]) -> BytesMut {
        let mut out = Vec::new();
        framing.encode(payload, &mut out);
        BytesMut::from(&out[..])
    }

    #[test]
    fn a_frame_waits_for_all_of_its_bytes() {
        let framing = Framing::default();
        let whole = wrapped(framing, &[7, 1, 2, 3]);
        for cut in 0..whole.len() {
            let mut partial = BytesMut::from(&whole[..cut]);
            assert_eq!(framing.decode(&mut partial), Ok(None));
        }
        let mut buffer = whole;
        assert_eq!(
            framing.decode(&mut buffer).unwrap().as_deref(),
            Some(&[7, 1, 2, 3][..])
        );
        assert!(buffer.is_empty());
    }

    #[test]
    fn compression_is_undone_on_both_sides_of_the_threshold() {
        let mut framing = Framing::default();
        framing.set_threshold(64);
        for payload in [vec![1u8; 10], vec![2u8; 5000]] {
            let mut buffer = wrapped(framing, &payload);
            assert_eq!(
                framing.decode(&mut buffer).unwrap().as_deref(),
                Some(&payload[..])
            );
        }
    }

    #[test]
    fn a_negative_threshold_turns_compression_off() {
        let mut framing = Framing::default();
        framing.set_threshold(256);
        assert!(framing.compressed());
        framing.set_threshold(-1);
        assert!(!framing.compressed());
    }

    #[test]
    fn a_length_of_garbage_is_an_error_not_a_panic() {
        let framing = Framing::default();
        let mut buffer = BytesMut::from(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01][..]);
        assert_eq!(framing.decode(&mut buffer), Err(WireError::BadLength));
        let mut buffer = BytesMut::from(&[0][..]);
        assert_eq!(framing.decode(&mut buffer), Err(WireError::BadLength));
    }

    #[test]
    fn a_frame_above_the_limit_is_refused_before_it_is_read() {
        let mut out = Vec::new();
        VarI32(MAX_FRAME_BYTES as i32 + 1).encode(&mut out);
        let mut buffer = BytesMut::from(&out[..]);
        assert!(matches!(
            Framing::default().decode(&mut buffer),
            Err(WireError::TooLarge(_))
        ));
    }

    #[test]
    fn a_compressed_frame_cannot_lie_about_its_size() {
        let mut framing = Framing::default();
        framing.set_threshold(1);
        let mut body = Vec::new();
        VarI32(10).encode(&mut body);
        let mut encoder = ZlibEncoder::new(body, Compression::fast());
        encoder.write_all(&[9u8; 4000]).unwrap();
        let body = encoder.finish().unwrap();
        let mut out = Vec::new();
        VarI32(body.len() as i32).encode(&mut out);
        out.extend_from_slice(&body);
        let mut buffer = BytesMut::from(&out[..]);
        assert_eq!(framing.decode(&mut buffer), Err(WireError::BadCompression));
    }
}
