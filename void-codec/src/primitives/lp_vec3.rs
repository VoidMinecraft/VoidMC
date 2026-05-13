use crate::{Decode, DecodeError, Encode, VarI32};

const DATA_BITS_MASK: u64 = 32767;
const MAX_QUANTIZED_VALUE: f64 = 32766.0;
const SCALE_BITS_MASK: u64 = 3;
const CONTINUATION_FLAG: u64 = 4;
const X_OFFSET: u64 = 3;
const Y_OFFSET: u64 = 18;
const Z_OFFSET: u64 = 33;
const ABS_MAX_VALUE: f64 = 1.7179869183E10;
const ABS_MIN_VALUE: f64 = 3.051944088384301E-5;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LpVec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl LpVec3 {
    pub const ZERO: LpVec3 = LpVec3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    fn sanitize(value: f64) -> f64 {
        if value.is_nan() {
            return 0.0;
        }
        value.clamp(-ABS_MAX_VALUE, ABS_MAX_VALUE)
    }

    fn pack(value: f64) -> u64 {
        ((value * 0.5 + 0.5) * MAX_QUANTIZED_VALUE).round() as u64
    }

    fn unpack(value: u64) -> f64 {
        (value & DATA_BITS_MASK).min(MAX_QUANTIZED_VALUE as u64) as f64 * 2.0 / MAX_QUANTIZED_VALUE
            - 1.0
    }

    fn has_continuation_bit(lowest: u8) -> bool {
        (lowest as u64 & CONTINUATION_FLAG) == CONTINUATION_FLAG
    }
}

impl Encode for LpVec3 {
    fn encode(&self, buf: &mut Vec<u8>) {
        let x = Self::sanitize(self.x);
        let y = Self::sanitize(self.y);
        let z = Self::sanitize(self.z);
        let chessboard_length = x.abs().max(y.abs()).max(z.abs());

        if chessboard_length < ABS_MIN_VALUE {
            buf.push(0);
        } else {
            let scale = chessboard_length.ceil() as u64;
            let is_partial = (scale & SCALE_BITS_MASK) != scale;
            let markers = if is_partial {
                scale & SCALE_BITS_MASK | CONTINUATION_FLAG
            } else {
                scale
            };
            let xn = Self::pack(x / scale as f64) << X_OFFSET;
            let yn = Self::pack(y / scale as f64) << Y_OFFSET;
            let zn = Self::pack(z / scale as f64) << Z_OFFSET;
            let buffer = markers | xn | yn | zn;

            buf.push((buffer & 0xFF) as u8);
            buf.push(((buffer >> 8) & 0xFF) as u8);
            buf.extend_from_slice(&((buffer >> 16) as u32).to_be_bytes());

            if is_partial {
                VarI32((scale >> 2) as i32).encode(buf);
            }
        }
    }
}

impl Decode for LpVec3 {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        if buf.is_empty() {
            return Err(DecodeError::UnexpectedEof);
        }

        let lowest = buf[0];
        *buf = &buf[1..];

        if lowest == 0 {
            return Ok(LpVec3::ZERO);
        }

        if buf.is_empty() {
            return Err(DecodeError::UnexpectedEof);
        }
        let middle = buf[0] as u64;
        *buf = &buf[1..];

        if buf.len() < 4 {
            return Err(DecodeError::UnexpectedEof);
        }
        let (bytes, rest) = buf.split_at(4);
        *buf = rest;
        let mut array = [0u8; 4];
        array.copy_from_slice(bytes);
        let highest = u32::from_be_bytes(array) as u64;

        let buffer = highest << 16 | middle << 8 | lowest as u64;
        let mut scale = lowest as u64 & SCALE_BITS_MASK;

        if Self::has_continuation_bit(lowest) {
            let continuation = VarI32::decode(buf)?;
            scale |= (continuation.0 as u64 & 0xFFFFFFFF) << 2;
        }

        Ok(LpVec3 {
            x: Self::unpack(buffer >> X_OFFSET) * scale as f64,
            y: Self::unpack(buffer >> Y_OFFSET) * scale as f64,
            z: Self::unpack(buffer >> Z_OFFSET) * scale as f64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lpvec3_zero() {
        let vec = LpVec3::ZERO;
        let mut buf = Vec::new();
        vec.encode(&mut buf);
        assert_eq!(buf, vec![0x00]);

        let mut slice = buf.as_slice();
        let decoded = LpVec3::decode(&mut slice).unwrap();
        assert_eq!(decoded, LpVec3::ZERO);
        assert_eq!(slice.len(), 0);
    }

    #[test]
    fn test_lpvec3_small_velocity() {
        let vec = LpVec3 {
            x: 0.5,
            y: 0.3,
            z: -0.2,
        };
        let mut buf = Vec::new();
        vec.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = LpVec3::decode(&mut slice).unwrap();

        assert!((decoded.x - vec.x).abs() < 0.01);
        assert!((decoded.y - vec.y).abs() < 0.01);
        assert!((decoded.z - vec.z).abs() < 0.01);
        assert_eq!(slice.len(), 0);
    }

    #[test]
    fn test_lpvec3_large_velocity() {
        let vec = LpVec3 {
            x: 100.0,
            y: 50.0,
            z: -75.0,
        };
        let mut buf = Vec::new();
        vec.encode(&mut buf);

        assert!(buf.len() > 1);

        let mut slice = buf.as_slice();
        let decoded = LpVec3::decode(&mut slice).unwrap();

        assert!((decoded.x - vec.x).abs() < 1.0);
        assert!((decoded.y - vec.y).abs() < 1.0);
        assert!((decoded.z - vec.z).abs() < 1.0);
        assert_eq!(slice.len(), 0);
    }

    #[test]
    fn test_lpvec3_negative_values() {
        let vec = LpVec3 {
            x: -10.0,
            y: -20.0,
            z: -5.0,
        };
        let mut buf = Vec::new();
        vec.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = LpVec3::decode(&mut slice).unwrap();

        assert!((decoded.x - vec.x).abs() < 0.5);
        assert!((decoded.y - vec.y).abs() < 0.5);
        assert!((decoded.z - vec.z).abs() < 0.5);
        assert_eq!(slice.len(), 0);
    }

    #[test]
    fn test_lpvec3_mixed_values() {
        let vec = LpVec3 {
            x: 15.5,
            y: -8.3,
            z: 0.0,
        };
        let mut buf = Vec::new();
        vec.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = LpVec3::decode(&mut slice).unwrap();

        assert!((decoded.x - vec.x).abs() < 0.5);
        assert!((decoded.y - vec.y).abs() < 0.5);
        assert!((decoded.z - vec.z).abs() < 0.01);
        assert_eq!(slice.len(), 0);
    }

    #[test]
    fn test_lpvec3_eof_handling() {
        let buf = Vec::new();
        let mut slice = buf.as_slice();
        assert_eq!(LpVec3::decode(&mut slice), Err(DecodeError::UnexpectedEof));
    }

    #[test]
    fn test_lpvec3_partial_decode() {
        let buf = vec![0x05];
        let mut slice = buf.as_slice();
        assert_eq!(LpVec3::decode(&mut slice), Err(DecodeError::UnexpectedEof));
    }

    #[test]
    fn test_lpvec3_nan_handling() {
        let vec = LpVec3 {
            x: f64::NAN,
            y: 1.0,
            z: 2.0,
        };
        let mut buf = Vec::new();
        vec.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = LpVec3::decode(&mut slice).unwrap();

        assert_eq!(decoded.x, 0.0);
    }
}
