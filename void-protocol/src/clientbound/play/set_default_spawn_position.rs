use voidmc_codec::{Decode, Encode};

use crate::types::BlockPosition;

/// The world spawn point (`minecraft:set_default_spawn_position`): the client
/// shows it as the world spawn and points compasses at it.
///
/// Vanilla writes its level's respawn data: the dimension and block of the
/// spawn (a global position), then the yaw and pitch a player respawns with.
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct SetDefaultSpawnPosition {
    /// Dimension identifier, e.g. `minecraft:overworld`.
    pub dimension: String,
    pub position: BlockPosition,
    pub yaw: f32,
    pub pitch: f32,
}

#[cfg(test)]
mod tests {
    use voidmc_codec::DecodeLimits;

    use super::*;
    use crate::clientbound::PlayPacket;

    fn spawn() -> SetDefaultSpawnPosition {
        SetDefaultSpawnPosition {
            dimension: "minecraft:overworld".to_string(),
            position: BlockPosition {
                x: -21,
                y: 90,
                z: 37,
            },
            yaw: 90.0,
            pitch: -12.5,
        }
    }

    /// The bytes Azalea 0.16.0 reads into `ClientboundSetDefaultSpawnPosition`:
    /// a `GlobalPos` (identifier, packed block position), then the yaw and
    /// the pitch as big-endian floats.
    const AZALEA_BYTES: [u8; 37] = [
        0x61, // packet id
        19, b'm', b'i', b'n', b'e', b'c', b'r', b'a', b'f', b't', b':', b'o', b'v', b'e', b'r',
        b'w', b'o', b'r', b'l', b'd', // dimension
        0xFF, 0xFF, 0xFA, 0xC0, 0x00, 0x02, 0x50, 0x5A, // x -21, z 37, y 90
        0x42, 0xB4, 0x00, 0x00, // yaw 90.0
        0xC1, 0x48, 0x00, 0x00, // pitch -12.5
    ];

    #[test]
    fn encodes_the_layout_azalea_reads() {
        let mut bytes = Vec::new();
        PlayPacket::SetDefaultSpawnPosition(spawn()).encode(&mut bytes);
        assert_eq!(bytes, AZALEA_BYTES);
    }

    #[test]
    fn decodes_what_it_encodes() {
        match DecodeLimits::default().decode_exact::<PlayPacket>(&AZALEA_BYTES) {
            Ok(PlayPacket::SetDefaultSpawnPosition(packet)) => assert_eq!(packet, spawn()),
            other => panic!("expected a default spawn position, got {other:?}"),
        }
    }
}
