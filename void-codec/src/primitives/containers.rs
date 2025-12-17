use crate::primitives::vari::VarI32;
use crate::{Decode, DecodeError, Encode};

impl<T: Encode> Encode for Vec<T> {
    fn encode(&self, buf: &mut Vec<u8>) {
        VarI32(self.len() as i32).encode(buf);
        for item in self {
            item.encode(buf);
        }
    }
}

impl<T: Decode> Decode for Vec<T> {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let len = VarI32::decode(buf)?.0;
        if len < 0 {
            return Err(DecodeError::InvalidLength);
        }

        let mut vec = Vec::with_capacity(len as usize);
        for _ in 0..len {
            vec.push(T::decode(buf)?);
        }
        Ok(vec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vec_i32_empty() {
        let value: Vec<i32> = vec![];
        let mut buf = Vec::new();
        value.encode(&mut buf);

        assert_eq!(
            buf.len(),
            1,
            "Empty vec should encode to single byte (length 0)"
        );

        let mut slice = buf.as_slice();
        let decoded = Vec::<i32>::decode(&mut slice).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_vec_i32_single() {
        let value: Vec<i32> = vec![12345];
        let mut buf = Vec::new();
        value.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = Vec::<i32>::decode(&mut slice).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_vec_i32_multiple() {
        let value: Vec<i32> = vec![1, 2, 3, 4, 5];
        let mut buf = Vec::new();
        value.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = Vec::<i32>::decode(&mut slice).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_vec_u8_bytes() {
        let value: Vec<u8> = vec![1, 2, 3, 255, 0, 127];
        let mut buf = Vec::new();
        value.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = Vec::<u8>::decode(&mut slice).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_vec_bool() {
        let value: Vec<bool> = vec![true, false, true, true, false];
        let mut buf = Vec::new();
        value.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = Vec::<bool>::decode(&mut slice).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_vec_large() {
        let value: Vec<i32> = (0..1000).collect();
        let mut buf = Vec::new();
        value.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = Vec::<i32>::decode(&mut slice).unwrap();
        assert_eq!(decoded, value);
        assert_eq!(slice.len(), 0);
    }

    #[test]
    fn test_vec_eof() {
        let mut slice = &[][..];
        let result = Vec::<i32>::decode(&mut slice);
        assert_eq!(result, Err(DecodeError::UnexpectedEof));
    }

    #[test]
    fn test_vec_invalid_length() {
        let mut buf = Vec::new();
        VarI32(-1).encode(&mut buf);

        let mut slice = buf.as_slice();
        let result = Vec::<i32>::decode(&mut slice);
        assert_eq!(result, Err(DecodeError::InvalidLength));
    }

    #[test]
    fn test_vec_empty_exact_bytes() {
        let value: Vec<i32> = vec![];
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf, vec![0x00]);
    }

    #[test]
    fn test_vec_u8_single_element() {
        let value: Vec<u8> = vec![42];
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf, vec![0x01, 42]);
    }

    #[test]
    fn test_vec_u8_exact_bytes() {
        let value: Vec<u8> = vec![1, 2, 3];
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf, vec![0x03, 1, 2, 3]);
    }

    #[test]
    fn test_vec_i32_exact_bytes() {
        let value: Vec<i32> = vec![256];
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf, vec![0x01, 0x00, 0x00, 0x01, 0x00]);
    }

    #[test]
    fn test_vec_bool_exact_bytes() {
        let value: Vec<bool> = vec![true, false, true];
        let mut buf = Vec::new();
        value.encode(&mut buf);
        assert_eq!(buf, vec![0x03, 0x01, 0x00, 0x01]);
    }

    #[test]
    fn test_vec_length_encoded_correctly() {
        for len in [0, 1, 5, 10, 127, 128, 255] {
            let value: Vec<i32> = (0..len).map(|i| i as i32).collect();
            let mut buf = Vec::new();
            value.encode(&mut buf);

            let mut slice = buf.as_slice();
            let decoded = Vec::<i32>::decode(&mut slice).unwrap();
            assert_eq!(decoded.len(), len, "Length {} not correctly encoded", len);
            assert_eq!(decoded, value);
        }
    }

    #[test]
    fn test_vec_nested_vecs() {
        let inner1: Vec<u8> = vec![1, 2];
        let inner2: Vec<u8> = vec![3, 4, 5];
        let value: Vec<Vec<u8>> = vec![inner1, inner2];

        let mut buf = Vec::new();
        value.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = Vec::<Vec<u8>>::decode(&mut slice).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_vec_u8_large_exact_format() {
        let value: Vec<u8> = vec![0xAA; 200];
        let mut buf = Vec::new();
        value.encode(&mut buf);

        assert_eq!(buf[0..2], [0xC8, 0x01]);
        for i in 2..buf.len() {
            assert_eq!(buf[i], 0xAA);
        }
    }

    #[test]
    fn test_vec_incomplete_data_error() {
        let mut buf = Vec::new();
        vec![1i32, 2i32, 3i32].encode(&mut buf);

        let mut slice = &buf[0..5];
        let result = Vec::<i32>::decode(&mut slice);
        assert!(result.is_err());
    }

    #[test]
    fn test_vec_roundtrip_mixed_types() {
        let values: Vec<(u8, bool)> = vec![(1, true), (255, false)];
        for (a, b) in values {
            let vec_a: Vec<u8> = vec![a];
            let vec_b: Vec<bool> = vec![b];

            let mut buf_a = Vec::new();
            vec_a.encode(&mut buf_a);
            let mut slice_a = buf_a.as_slice();
            let decoded_a = Vec::<u8>::decode(&mut slice_a).unwrap();
            assert_eq!(decoded_a, vec_a);

            let mut buf_b = Vec::new();
            vec_b.encode(&mut buf_b);
            let mut slice_b = buf_b.as_slice();
            let decoded_b = Vec::<bool>::decode(&mut slice_b).unwrap();
            assert_eq!(decoded_b, vec_b);
        }
    }
}
