use std::io::{self, Read, Write};

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use voidmc_codec::{Decode, Encode, VarI32};

pub const MAX_FRAME: usize = (1 << 21) - 1;
pub const MAX_PACKET: usize = 1 << 23;

fn invalid(why: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why)
}

fn frame_length(stream: &mut impl Read) -> io::Result<usize> {
    let mut length = 0;
    for at in 0..3 {
        let mut byte = [0u8];
        stream.read_exact(&mut byte)?;
        length |= usize::from(byte[0] & 0x7F) << (7 * at);
        if byte[0] & 0x80 == 0 {
            return Ok(length);
        }
    }
    Err(invalid("a frame length longer than 3 bytes".into()))
}

/// The one way to read a packet: bounded like the game bounds it, and decompressed once
/// compression is on.
pub fn read(stream: &mut impl Read, threshold: Option<usize>) -> io::Result<Vec<u8>> {
    let length = frame_length(stream)?;
    let mut frame = vec![0; length];
    stream.read_exact(&mut frame)?;
    if threshold.is_none() {
        return Ok(frame);
    }
    let mut rest = frame.as_slice();
    let size = VarI32::decode(&mut rest)
        .map_err(|e| invalid(format!("{e:?}")))?
        .0;
    let size = usize::try_from(size).map_err(|_| invalid(format!("a packet of {size} bytes")))?;
    if size == 0 {
        return Ok(rest.to_vec());
    }
    if size > MAX_PACKET {
        return Err(invalid(format!(
            "a packet of {size} bytes, the limit is {MAX_PACKET}"
        )));
    }
    let mut body = Vec::with_capacity(size);
    ZlibDecoder::new(rest)
        .take(size as u64 + 1)
        .read_to_end(&mut body)?;
    if body.len() != size {
        return Err(invalid(format!(
            "a packet announced {size} bytes and holds {}",
            body.len()
        )));
    }
    Ok(body)
}

pub fn encode(body: &[u8], threshold: Option<usize>) -> io::Result<Vec<u8>> {
    if body.len() > MAX_PACKET {
        return Err(invalid(format!("a packet of {} bytes", body.len())));
    }
    let inner = match threshold {
        None => body.to_vec(),
        Some(threshold) if body.len() < threshold => {
            let mut inner = Vec::with_capacity(body.len() + 1);
            VarI32(0).encode(&mut inner);
            inner.extend_from_slice(body);
            inner
        }
        Some(_) => {
            let mut inner = Vec::new();
            VarI32(body.len() as i32).encode(&mut inner);
            let mut zlib = ZlibEncoder::new(inner, Compression::default());
            zlib.write_all(body)?;
            zlib.finish()?
        }
    };
    if inner.len() > MAX_FRAME {
        return Err(invalid(format!("a frame of {} bytes", inner.len())));
    }
    let mut frame = Vec::with_capacity(inner.len() + 3);
    VarI32(inner.len() as i32).encode(&mut frame);
    frame.extend(inner);
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(result: io::Result<Vec<u8>>) -> io::ErrorKind {
        result.unwrap_err().kind()
    }

    #[test]
    fn a_packet_comes_back_as_it_left_with_or_without_compression() {
        let small = vec![7u8; 10];
        let large: Vec<u8> = (0..5000u32).map(|n| (n % 251) as u8).collect();
        for threshold in [None, Some(256)] {
            for body in [&small, &large] {
                let frame = encode(body, threshold).unwrap();
                assert_eq!(&read(&mut frame.as_slice(), threshold).unwrap(), body);
            }
        }
        let compressed = encode(&large, Some(256)).unwrap();
        assert!(compressed.len() < large.len());
    }

    #[test]
    fn a_length_longer_than_three_bytes_is_refused_without_reading_on() {
        let frame = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x0F];
        assert_eq!(
            kind(read(&mut &frame[..], None)),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn a_frame_shorter_than_its_length_is_an_error() {
        let frame = [0x05, 1, 2];
        assert_eq!(
            kind(read(&mut &frame[..], None)),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn a_compressed_packet_must_hold_what_it_announces() {
        let mut lying = encode(&[1u8; 400], Some(256)).unwrap();
        assert_eq!(lying[1..3], [0x90, 0x03]);
        lying[1] = 0x91;
        assert_eq!(
            kind(read(&mut lying.as_slice(), Some(256))),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn a_packet_announced_beyond_the_limit_is_refused() {
        let mut inner = Vec::new();
        VarI32((MAX_PACKET + 1) as i32).encode(&mut inner);
        inner.extend([0x78, 0x9C]);
        let mut frame = Vec::new();
        VarI32(inner.len() as i32).encode(&mut frame);
        frame.extend(inner);
        assert_eq!(
            kind(read(&mut frame.as_slice(), Some(256))),
            io::ErrorKind::InvalidData
        );
    }
}
