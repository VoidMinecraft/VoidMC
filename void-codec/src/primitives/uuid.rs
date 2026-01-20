use crate::{Decode, DecodeError, Encode};
use uuid::Uuid;

impl Encode for Uuid {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self.as_bytes());
    }
}

impl Decode for Uuid {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        if buf.len() < 16 {
            return Err(DecodeError::UnexpectedEof);
        }

        let uuid_bytes = &buf[..16];
        *buf = &buf[16..];

        Ok(Uuid::from_bytes(uuid_bytes.try_into().unwrap()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uuid_encode_decode() {
        let uuid = Uuid::new_v4();
        let mut buf = Vec::new();
        uuid.encode(&mut buf);

        assert_eq!(buf.len(), 16, "UUID should encode to 16 bytes");

        let mut slice = buf.as_slice();
        let decoded = Uuid::decode(&mut slice).unwrap();

        assert_eq!(decoded, uuid);
        assert_eq!(slice.len(), 0, "All bytes should be consumed");
    }

    #[test]
    fn test_uuid_exact_bytes() {
        let uuid = Uuid::nil();
        let mut buf = Vec::new();
        uuid.encode(&mut buf);

        assert_eq!(
            buf,
            vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            "Nil UUID should encode as all zeros"
        );
    }

    #[test]
    fn test_uuid_truncated() {
        let mut slice = &[0u8; 15][..];
        let result = Uuid::decode(&mut slice);

        assert_eq!(result, Err(DecodeError::UnexpectedEof));
    }

    #[test]
    fn test_uuid_known_value() {
        let uuid_str = "f47ac10b-58cc-4372-a567-0e02b2c3d479";
        let uuid = uuid_str.parse::<Uuid>().unwrap();

        let mut buf = Vec::new();
        uuid.encode(&mut buf);

        let mut slice = buf.as_slice();
        let decoded = Uuid::decode(&mut slice).unwrap();

        assert_eq!(decoded.to_string(), uuid_str);
    }

    #[test]
    fn test_uuid_roundtrip_multiple() {
        for _ in 0..100 {
            let uuid = Uuid::new_v4();
            let mut buf = Vec::new();
            uuid.encode(&mut buf);

            let mut slice = buf.as_slice();
            let decoded = Uuid::decode(&mut slice).unwrap();

            assert_eq!(decoded, uuid);
        }
    }
}
