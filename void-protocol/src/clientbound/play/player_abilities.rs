use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Copy, PartialEq, Encode, Decode)]
pub struct PlayerAbilities {
    pub flags: u8,
    pub flying_speed: f32,
    pub walking_speed: f32,
}

impl PlayerAbilities {
    pub const INVULNERABLE: u8 = 0x01;
    pub const FLYING: u8 = 0x02;
    pub const ALLOW_FLYING: u8 = 0x04;
    pub const INSTANT_BUILD: u8 = 0x08;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flying_abilities_wire_layout() {
        let mut bytes = Vec::new();
        super::super::PlayPacket::PlayerAbilities(PlayerAbilities {
            flags: 7,
            flying_speed: 0.05,
            walking_speed: 0.1,
        })
        .encode(&mut bytes);
        assert_eq!(
            bytes,
            [0x40, 7, 0x3d, 0x4c, 0xcc, 0xcd, 0x3d, 0xcc, 0xcc, 0xcd]
        );
    }
}
