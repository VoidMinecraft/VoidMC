use crate::{Decode, DecodeError, Encode};

impl Encode for bool {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.push(if *self { 1 } else { 0 });
    }
}

impl Decode for bool {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        if buf.is_empty() {
            return Err(DecodeError::UnexpectedEof);
        }

        let value = buf[0] != 0;
        *buf = &buf[1..];
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bool_true() {
        let value = true;
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf, vec![1]);

        let mut slice = buf.as_slice();
        let decoded = bool::decode(&mut slice).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_bool_false() {
        let value = false;
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf, vec![0]);

        let mut slice = buf.as_slice();
        let decoded = bool::decode(&mut slice).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_bool_exact_encoding() {
        let mut buf = Vec::new();
        true.encode(&mut buf);
        assert_eq!(buf, vec![1]);

        buf.clear();
        false.encode(&mut buf);
        assert_eq!(buf, vec![0]);
    }

    #[test]
    fn test_bool_only_zero_decodes_false() {
        let mut slice = &[0u8][..];
        let decoded = bool::decode(&mut slice).unwrap();
        assert!(!decoded);
    }

    #[test]
    fn test_bool_non_zero_decodes_true() {
        for byte in 1u8..=255u8 {
            let mut slice = &[byte][..];
            let decoded = bool::decode(&mut slice).unwrap();
            assert!(decoded, "byte {} should decode to true", byte);
        }
    }

    #[test]
    fn test_bool_sequence() {
        let values = [true, false, true, true, false];
        let mut buf = Vec::new();
        for &val in &values {
            val.encode(&mut buf);
        }

        assert_eq!(buf.len(), 5);
        assert_eq!(buf, vec![1, 0, 1, 1, 0]);
    }
}
