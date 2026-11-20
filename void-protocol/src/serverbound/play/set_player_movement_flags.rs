use voidmc_codec::{Decode, Encode};

/// Set Player Movement Flags (`minecraft:move_player_status_only`, 0x21): the
/// movement packet a client sends when neither its position nor its rotation
/// changed but its on-ground or horizontal-collision state did.
///
/// The payload is the flags byte of the other movement packets: bit 0 is
/// on ground, bit 1 is horizontal collision.
#[derive(Debug, Encode, Decode)]
pub struct SetPlayerMovementFlags {
    pub flags: u8,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serverbound::PlayPacket;

    /// The bytes Azalea 0.16.0 writes for `ServerboundMovePlayerStatusOnly`
    /// on the ground and against a wall: the packet id, then a one-byte
    /// `FixedBitSet<8>` with bit 0 (on ground) and bit 1 (horizontal
    /// collision).
    #[test]
    fn decodes_the_tagged_packet_as_azalea_writes_it() {
        for flags in [0x00, 0x01, 0x02, 0x03] {
            let mut slice = &[0x21, flags][..];
            let PlayPacket::SetPlayerMovementFlags(packet) =
                PlayPacket::decode(&mut slice).unwrap()
            else {
                panic!("decoded another packet");
            };
            assert!(slice.is_empty());
            assert_eq!(packet.flags, flags);
        }
    }

    #[test]
    fn encodes_one_flags_byte() {
        let mut buf = Vec::new();
        SetPlayerMovementFlags { flags: 0x01 }.encode(&mut buf);
        assert_eq!(buf, [0x01]);
    }
}
