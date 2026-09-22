use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct InitializeBorder {
    pub center_x: f64,
    pub center_z: f64,
    pub old_diameter: f64,
    pub new_diameter: f64,
    #[codec(varint64)]
    pub lerp_ticks: i64,
    #[codec(varint32)]
    pub portal_teleport_boundary: i32,
    #[codec(varint32)]
    pub warning_blocks: i32,
    #[codec(varint32)]
    pub warning_ticks: i32,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct SetBorderCenter {
    pub center_x: f64,
    pub center_z: f64,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct SetBorderLerpSize {
    pub old_diameter: f64,
    pub new_diameter: f64,
    #[codec(varint64)]
    pub lerp_ticks: i64,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct SetBorderSize {
    pub diameter: f64,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct SetBorderWarningDelay {
    #[codec(varint32)]
    pub warning_ticks: i32,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct SetBorderWarningDistance {
    #[codec(varint32)]
    pub warning_blocks: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::PlayPacket;

    fn encode(packet: PlayPacket) -> Vec<u8> {
        let mut bytes = Vec::new();
        packet.encode(&mut bytes);
        bytes
    }

    #[test]
    fn initialize_border_matches_paper_layout() {
        let bytes = encode(PlayPacket::InitializeBorder(InitializeBorder {
            center_x: -128.5,
            center_z: 16.25,
            old_diameter: 512.0,
            new_diameter: 64.0,
            lerp_ticks: 2400,
            portal_teleport_boundary: 29999984,
            warning_blocks: 8,
            warning_ticks: 300,
        }));
        let mut expected = vec![0x2B];
        expected.extend((-128.5f64).to_be_bytes());
        expected.extend(16.25f64.to_be_bytes());
        expected.extend(512.0f64.to_be_bytes());
        expected.extend(64.0f64.to_be_bytes());
        expected.extend([0xE0, 0x12]);
        expected.extend([0xF0, 0x86, 0xA7, 0x0E]);
        expected.extend([0x08]);
        expected.extend([0xAC, 0x02]);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn initialize_border_static_default_world() {
        let bytes = encode(PlayPacket::InitializeBorder(InitializeBorder {
            center_x: 0.0,
            center_z: 0.0,
            old_diameter: 59999968.0,
            new_diameter: 59999968.0,
            lerp_ticks: 0,
            portal_teleport_boundary: 29999984,
            warning_blocks: 5,
            warning_ticks: 300,
        }));
        let mut expected = vec![0x2B];
        expected.extend([0u8; 16]);
        expected.extend([0x41, 0x8C, 0x9C, 0x37, 0, 0, 0, 0]);
        expected.extend([0x41, 0x8C, 0x9C, 0x37, 0, 0, 0, 0]);
        expected.extend([0x00]);
        expected.extend([0xF0, 0x86, 0xA7, 0x0E]);
        expected.extend([0x05]);
        expected.extend([0xAC, 0x02]);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn set_border_center_is_two_doubles() {
        let bytes = encode(PlayPacket::SetBorderCenter(SetBorderCenter {
            center_x: 512.0,
            center_z: -128.5,
        }));
        let mut expected = vec![0x58];
        expected.extend(512.0f64.to_be_bytes());
        expected.extend((-128.5f64).to_be_bytes());
        assert_eq!(bytes, expected);
    }

    #[test]
    fn set_border_lerp_size_is_two_doubles_and_a_varlong() {
        let bytes = encode(PlayPacket::SetBorderLerpSize(SetBorderLerpSize {
            old_diameter: 512.0,
            new_diameter: 64.0,
            lerp_ticks: 6_000_000,
        }));
        let mut expected = vec![0x59];
        expected.extend(512.0f64.to_be_bytes());
        expected.extend(64.0f64.to_be_bytes());
        expected.extend([0x80, 0x9B, 0xEE, 0x02]);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn set_border_size_is_one_double() {
        let bytes = encode(PlayPacket::SetBorderSize(SetBorderSize { diameter: 64.0 }));
        let mut expected = vec![0x5A];
        expected.extend(64.0f64.to_be_bytes());
        assert_eq!(bytes, expected);
    }

    #[test]
    fn warning_delay_and_distance_are_varints() {
        assert_eq!(
            encode(PlayPacket::SetBorderWarningDelay(SetBorderWarningDelay {
                warning_ticks: 300
            })),
            vec![0x5B, 0xAC, 0x02]
        );
        assert_eq!(
            encode(PlayPacket::SetBorderWarningDistance(
                SetBorderWarningDistance { warning_blocks: 8 }
            )),
            vec![0x5C, 0x08]
        );
    }
}
