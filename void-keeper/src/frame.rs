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
use ussr_nbt::owned::{Nbt, Tag};
use voidmc_codec::{Decode, Encode, VarI32};
use voidmc_net::socket::{FrameLength, frame_length};

pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
/// What a client reads on a link without compression: the length of a frame takes three bytes.
pub const MAX_PLAIN_FRAME_BYTES: usize = (1 << 21) - 1;

#[derive(Debug, PartialEq, Eq)]
pub enum FrameError {
    Length(voidmc_net::socket::FrameError),
    InflatedTooLarge(usize),
    BadCompression,
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length(error) => error.fmt(f),
            Self::InflatedTooLarge(len) => {
                write!(f, "inflated frame of {len} bytes is above the limit")
            }
            Self::BadCompression => write!(f, "invalid compressed frame"),
        }
    }
}

impl std::error::Error for FrameError {}

/// How one side of the proxy frames its packets.
#[derive(Debug, Default, Clone, Copy)]
pub struct FrameCodec {
    threshold: Option<usize>,
}

impl FrameCodec {
    /// A negative threshold turns compression off, as `login_compression` defines it.
    pub fn set_threshold(&mut self, threshold: i32) {
        self.threshold = usize::try_from(threshold).ok();
    }

    /// The next whole frame of `buffer`, or `None` until more bytes arrive.
    pub fn decode(&self, buffer: &mut BytesMut) -> Result<Option<Bytes>, FrameError> {
        let Some(FrameLength { prefix, body }) =
            frame_length(buffer, MAX_FRAME_BYTES).map_err(FrameError::Length)?
        else {
            return Ok(None);
        };
        if buffer.len() < prefix + body {
            buffer.reserve(prefix + body - buffer.len());
            return Ok(None);
        }
        buffer.advance(prefix);
        let frame = buffer.split_to(body).freeze();
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

fn inflate(frame: Bytes) -> Result<Bytes, FrameError> {
    let mut rest: &[u8] = &frame;
    let data_length = VarI32::decode(&mut rest)
        .map_err(|_| FrameError::BadCompression)?
        .0;
    let consumed = frame.len() - rest.len();
    if data_length == 0 {
        return Ok(frame.slice(consumed..));
    }
    let expected = usize::try_from(data_length).map_err(|_| FrameError::BadCompression)?;
    if expected > MAX_FRAME_BYTES {
        return Err(FrameError::InflatedTooLarge(expected));
    }
    let mut inflated = Vec::with_capacity(expected);
    ZlibDecoder::new(rest)
        .take(expected as u64 + 1)
        .read_to_end(&mut inflated)
        .map_err(|_| FrameError::BadCompression)?;
    if inflated.len() != expected {
        return Err(FrameError::BadCompression);
    }
    Ok(inflated.into())
}

/// A frame whose packet id was read once.
#[derive(Debug, Clone)]
pub struct Frame {
    bytes: Bytes,
    id: i32,
    body_at: usize,
}

impl Frame {
    pub(crate) fn read(bytes: Bytes) -> Option<Self> {
        let (id, body) = Self::id_and_body(&bytes)?;
        let body_at = bytes.len() - body.len();
        Some(Self { bytes, id, body_at })
    }

    /// The packet id of a frame, and what follows it.
    pub fn id_and_body(frame: &[u8]) -> Option<(i32, &[u8])> {
        let mut body = frame;
        let id = VarI32::decode(&mut body).ok()?.0;
        Some((id, body))
    }

    /// A frame whose body is written by hand, for the packets `voidmc-protocol` has no type for.
    pub fn encode(id: i32, write: impl FnOnce(&mut Vec<u8>)) -> Bytes {
        let mut buffer = Vec::new();
        VarI32(id).encode(&mut buffer);
        write(&mut buffer);
        buffer.into()
    }

    pub(crate) fn id(&self) -> i32 {
        self.id
    }

    pub(crate) fn body(&self) -> &[u8] {
        &self.bytes[self.body_at..]
    }

    pub(crate) fn bytes(&self) -> &Bytes {
        &self.bytes
    }

    pub(crate) fn into_bytes(self) -> Bytes {
        self.bytes
    }
}

/// A plain text component, the way the client reads it from a play packet.
pub fn text(content: &str) -> Nbt {
    Nbt {
        name: "".into(),
        compound: vec![("text".into(), Tag::String(content.into()))].into(),
    }
}

#[cfg(test)]
mod tests {
    use voidmc_net::socket::FrameError as NetFrameError;

    use super::*;

    fn wrapped(codec: FrameCodec, payload: &[u8]) -> BytesMut {
        let mut out = Vec::new();
        codec.encode(payload, &mut out);
        BytesMut::from(&out[..])
    }

    #[test]
    fn a_frame_waits_for_all_of_its_bytes() {
        let codec = FrameCodec::default();
        let whole = wrapped(codec, &[7, 1, 2, 3]);
        for cut in 0..whole.len() {
            let mut partial = BytesMut::from(&whole[..cut]);
            assert_eq!(codec.decode(&mut partial), Ok(None));
        }
        let mut buffer = whole;
        assert_eq!(
            codec.decode(&mut buffer).unwrap().as_deref(),
            Some(&[7, 1, 2, 3][..])
        );
        assert!(buffer.is_empty());
    }

    #[test]
    fn compression_is_undone_on_both_sides_of_the_threshold() {
        let mut codec = FrameCodec::default();
        codec.set_threshold(64);
        for payload in [vec![1u8; 10], vec![2u8; 5000]] {
            let mut buffer = wrapped(codec, &payload);
            assert_eq!(
                codec.decode(&mut buffer).unwrap().as_deref(),
                Some(&payload[..])
            );
        }
    }

    #[test]
    fn a_negative_threshold_turns_compression_off() {
        let mut codec = FrameCodec::default();
        codec.set_threshold(256);
        codec.set_threshold(-1);
        let mut out = Vec::new();
        codec.encode(&[7, 1], &mut out);
        assert_eq!(out, [2, 7, 1]);
    }

    #[test]
    fn a_length_of_garbage_is_an_error_not_a_panic() {
        let codec = FrameCodec::default();
        let mut buffer = BytesMut::from(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01][..]);
        assert_eq!(
            codec.decode(&mut buffer),
            Err(FrameError::Length(NetFrameError::InvalidLengthVarInt))
        );
        let mut buffer = BytesMut::from(&[0][..]);
        assert_eq!(
            codec.decode(&mut buffer),
            Err(FrameError::Length(NetFrameError::EmptyFrame))
        );
    }

    #[test]
    fn a_fifth_length_byte_with_high_bits_is_refused_as_varints_are() {
        let mut buffer = BytesMut::from(&[0x81, 0x80, 0x80, 0x80, 0x10, 7][..]);
        assert_eq!(
            FrameCodec::default().decode(&mut buffer),
            Err(FrameError::Length(NetFrameError::InvalidLengthVarInt))
        );
    }

    #[test]
    fn a_frame_above_the_limit_is_refused_before_it_is_read() {
        let mut out = Vec::new();
        VarI32(MAX_FRAME_BYTES as i32 + 1).encode(&mut out);
        let mut buffer = BytesMut::from(&out[..]);
        assert!(matches!(
            FrameCodec::default().decode(&mut buffer),
            Err(FrameError::Length(NetFrameError::FrameTooLarge {
                limit: MAX_FRAME_BYTES,
                ..
            }))
        ));
    }

    #[test]
    fn a_compressed_frame_cannot_lie_about_its_size() {
        let mut codec = FrameCodec::default();
        codec.set_threshold(1);
        let mut body = Vec::new();
        VarI32(10).encode(&mut body);
        let mut encoder = ZlibEncoder::new(body, Compression::fast());
        encoder.write_all(&[9u8; 4000]).unwrap();
        let body = encoder.finish().unwrap();
        let mut out = Vec::new();
        VarI32(body.len() as i32).encode(&mut out);
        out.extend_from_slice(&body);
        let mut buffer = BytesMut::from(&out[..]);
        assert_eq!(codec.decode(&mut buffer), Err(FrameError::BadCompression));
    }
}
