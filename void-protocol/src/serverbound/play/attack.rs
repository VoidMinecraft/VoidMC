use voidmc_codec::{Decode, DecodeError, Decoder, Encode, VarI32};

/// A left-click attack on an entity (`minecraft:attack`), split out of
/// Interact in 26.1.
///
/// Vanilla writes the entity id as a VarInt, and so does [`Encode`]. Decoding
/// also accepts a 4-byte big-endian int, the form Azalea 0.16.0 writes (fixed
/// upstream in azalea-rs/azalea#323). The two cannot be confused: a 4-byte
/// VarInt has the continuation bit set on its first byte, which as an int
/// would be a negative entity id. The packet is the whole frame, so the
/// decoder reads every byte left in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attack {
    pub entity_id: i32,
}

impl Encode for Attack {
    fn encode(&self, buf: &mut Vec<u8>) {
        VarI32(self.entity_id).encode(buf);
    }
}

impl Decode for Attack {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let rest = decoder.remaining();
        if let Ok(VarI32(entity_id)) = decoder.limits().decode_exact::<VarI32>(rest) {
            decoder.take(rest.len())?;
            return Ok(Self { entity_id });
        }
        if rest.len() == 4 {
            let entity_id = i32::from_be_bytes(decoder.take_array::<4>()?);
            return Ok(Self { entity_id });
        }
        let VarI32(entity_id) = decoder.decode()?;
        Ok(Self { entity_id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serverbound::PlayPacket;
    use voidmc_codec::DecodeLimits;

    fn decode(bytes: &[u8]) -> Result<PlayPacket, DecodeError> {
        DecodeLimits::default().decode_exact::<PlayPacket>(bytes)
    }

    fn attacked(bytes: &[u8]) -> i32 {
        match decode(bytes) {
            Ok(PlayPacket::Attack(attack)) => attack.entity_id,
            other => panic!("expected an attack packet, got {other:?}"),
        }
    }

    #[test]
    fn decodes_the_vanilla_varint_entity_id() {
        assert_eq!(attacked(&[0x01, 42]), 42);
        assert_eq!(attacked(&[0x01, 0xAC, 0x02]), 300);
        // A VarInt that takes four bytes is still read as a VarInt.
        assert_eq!(attacked(&[0x01, 0x80, 0x80, 0x80, 0x01]), 1 << 21);
    }

    #[test]
    fn decodes_the_int_entity_id_azalea_0_16_writes() {
        assert_eq!(attacked(&[0x01, 0, 0, 0, 42]), 42);
        assert_eq!(attacked(&[0x01, 0, 0, 0x01, 0x2C]), 300);
    }

    #[test]
    fn encodes_the_entity_id_as_a_varint() {
        let mut bytes = Vec::new();
        Attack { entity_id: 300 }.encode(&mut bytes);
        assert_eq!(bytes, [0xAC, 0x02]);
    }

    #[test]
    fn rejects_an_empty_or_overlong_body() {
        assert!(decode(&[0x01]).is_err());
        assert!(decode(&[0x01, 42, 0]).is_err());
        assert!(decode(&[0x01, 0, 0, 0, 42, 0]).is_err());
    }
}
