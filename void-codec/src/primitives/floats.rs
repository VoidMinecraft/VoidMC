use crate::{Decode, DecodeError, Decoder, Encode};

impl Encode for f32 {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_be_bytes());
    }
}

impl Decode for f32 {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let bytes = decoder.take(4)?;
        let mut array = [0u8; 4];
        array.copy_from_slice(bytes);
        Ok(f32::from_be_bytes(array))
    }
}

impl Encode for f64 {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_be_bytes());
    }
}

impl Decode for f64 {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let bytes = decoder.take(8)?;
        let mut array = [0u8; 8];
        array.copy_from_slice(bytes);
        Ok(f64::from_be_bytes(array))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_f32_roundtrip() {
        let value = std::f32::consts::PI;
        let mut buf = Vec::new();
        value.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = f32::decode(&mut slice).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_f64_roundtrip() {
        let value = std::f64::consts::PI;
        let mut buf = Vec::new();
        value.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = f64::decode(&mut slice).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_f32_exact_bytes_zero() {
        let mut buf = Vec::new();
        (0.0f32).encode(&mut buf);
        assert_eq!(buf, vec![0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_f32_exact_bytes_one() {
        let mut buf = Vec::new();
        (1.0f32).encode(&mut buf);
        assert_eq!(buf, vec![0x3F, 0x80, 0x00, 0x00]);
    }

    #[test]
    fn test_f32_exact_bytes_negative_one() {
        let mut buf = Vec::new();
        (-1.0f32).encode(&mut buf);
        assert_eq!(buf, vec![0xBF, 0x80, 0x00, 0x00]);
    }

    #[test]
    fn test_f32_exact_bytes_pi() {
        let mut buf = Vec::new();
        (std::f32::consts::PI).encode(&mut buf);
        assert_eq!(buf, vec![0x40, 0x49, 0x0F, 0xDB]);
    }

    #[test]
    fn test_f32_infinity() {
        let mut buf = Vec::new();
        (f32::INFINITY).encode(&mut buf);
        assert_eq!(buf, vec![0x7F, 0x80, 0x00, 0x00]);
    }

    #[test]
    fn test_f32_neg_infinity() {
        let mut buf = Vec::new();
        (f32::NEG_INFINITY).encode(&mut buf);
        assert_eq!(buf, vec![0xFF, 0x80, 0x00, 0x00]);
    }

    #[test]
    fn test_f64_exact_bytes_zero() {
        let mut buf = Vec::new();
        (0.0f64).encode(&mut buf);
        assert_eq!(buf, vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_f64_exact_bytes_one() {
        let mut buf = Vec::new();
        (1.0f64).encode(&mut buf);
        assert_eq!(buf, vec![0x3F, 0xF0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_f64_exact_bytes_negative_one() {
        let mut buf = Vec::new();
        (-1.0f64).encode(&mut buf);
        assert_eq!(buf, vec![0xBF, 0xF0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_f64_exact_bytes_pi() {
        let mut buf = Vec::new();
        (std::f64::consts::PI).encode(&mut buf);
        assert_eq!(buf, vec![0x40, 0x09, 0x21, 0xFB, 0x54, 0x44, 0x2D, 0x18]);
    }

    #[test]
    fn test_f64_infinity() {
        let mut buf = Vec::new();
        (f64::INFINITY).encode(&mut buf);
        assert_eq!(buf, vec![0x7F, 0xF0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_f64_neg_infinity() {
        let mut buf = Vec::new();
        (f64::NEG_INFINITY).encode(&mut buf);
        assert_eq!(buf, vec![0xFF, 0xF0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_f32_small_positive() {
        let value = 0.5f32;
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf, vec![0x3F, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_f64_small_positive() {
        let value = 0.5f64;
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf, vec![0x3F, 0xE0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    }
}
