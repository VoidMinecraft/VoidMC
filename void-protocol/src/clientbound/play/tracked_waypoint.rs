use uuid::Uuid;
use voidmc_codec::{Decode, DecodeError, Decoder, Encode, VarI32};

pub const DEFAULT_WAYPOINT_STYLE: &str = "minecraft:default";

/// Clientbound `waypoint` packet (`ClientboundTrackedWaypointPacket`): adds,
/// moves or removes one entry of the client's locator bar. The client crashes
/// on an `Update` for an id it does not track, and `Update` only moves the
/// waypoint; send `Track` again to change its icon or kind.
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct TrackedWaypoint {
    pub operation: WaypointOperation,
    pub waypoint: Waypoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
#[codec(varint32)]
#[repr(i32)]
pub enum WaypointOperation {
    Track = 0,
    Untrack = 1,
    Update = 2,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct Waypoint {
    pub id: WaypointId,
    pub icon: WaypointIcon,
    pub position: WaypointPosition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaypointId {
    Uuid(Uuid),
    Name(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct WaypointIcon {
    pub style: String,
    pub color: Option<WaypointColor>,
}

impl Default for WaypointIcon {
    fn default() -> Self {
        Self {
            style: DEFAULT_WAYPOINT_STYLE.to_string(),
            color: None,
        }
    }
}

/// Three raw bytes on the wire, without alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaypointColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl WaypointColor {
    pub const fn rgb(rgb: u32) -> Self {
        Self {
            red: (rgb >> 16) as u8,
            green: (rgb >> 8) as u8,
            blue: rgb as u8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WaypointPosition {
    Empty,
    Block { x: i32, y: i32, z: i32 },
    Chunk { x: i32, z: i32 },
    Azimuth(f32),
}

impl Waypoint {
    /// The body vanilla sends with `Untrack`: the id with the default icon
    /// and no position.
    pub fn untracked(id: WaypointId) -> Self {
        Self {
            id,
            icon: WaypointIcon::default(),
            position: WaypointPosition::Empty,
        }
    }
}

impl Encode for WaypointId {
    fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            WaypointId::Uuid(uuid) => {
                true.encode(buf);
                uuid.encode(buf);
            }
            WaypointId::Name(name) => {
                false.encode(buf);
                name.encode(buf);
            }
        }
    }
}

impl Decode for WaypointId {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        Ok(if decoder.decode::<bool>()? {
            WaypointId::Uuid(decoder.decode()?)
        } else {
            WaypointId::Name(decoder.decode()?)
        })
    }
}

impl Encode for WaypointColor {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&[self.red, self.green, self.blue]);
    }
}

impl Decode for WaypointColor {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            red: decoder.decode()?,
            green: decoder.decode()?,
            blue: decoder.decode()?,
        })
    }
}

impl Encode for WaypointPosition {
    fn encode(&self, buf: &mut Vec<u8>) {
        match *self {
            WaypointPosition::Empty => VarI32(0).encode(buf),
            WaypointPosition::Block { x, y, z } => {
                VarI32(1).encode(buf);
                VarI32(x).encode(buf);
                VarI32(y).encode(buf);
                VarI32(z).encode(buf);
            }
            WaypointPosition::Chunk { x, z } => {
                VarI32(2).encode(buf);
                VarI32(x).encode(buf);
                VarI32(z).encode(buf);
            }
            WaypointPosition::Azimuth(angle) => {
                VarI32(3).encode(buf);
                angle.encode(buf);
            }
        }
    }
}

impl Decode for WaypointPosition {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        Ok(match decoder.decode::<VarI32>()?.0 {
            0 => WaypointPosition::Empty,
            1 => WaypointPosition::Block {
                x: decoder.decode::<VarI32>()?.0,
                y: decoder.decode::<VarI32>()?.0,
                z: decoder.decode::<VarI32>()?.0,
            },
            2 => WaypointPosition::Chunk {
                x: decoder.decode::<VarI32>()?.0,
                z: decoder.decode::<VarI32>()?.0,
            },
            3 => WaypointPosition::Azimuth(decoder.decode()?),
            _ => return Err(DecodeError::InvalidPacketId(None)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::PlayPacket;

    fn id() -> WaypointId {
        WaypointId::Uuid(Uuid::from_u128(0x0000_0000_0000_0001_0000_0000_0000_0002))
    }

    fn encode(packet: TrackedWaypoint) -> Vec<u8> {
        let mut bytes = Vec::new();
        PlayPacket::TrackedWaypoint(packet).encode(&mut bytes);
        bytes
    }

    fn hex(text: &str) -> Vec<u8> {
        let digits: String = text.split_whitespace().collect();
        (0..digits.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&digits[i..i + 2], 16).expect("hex"))
            .collect()
    }

    #[test]
    fn track_matches_vanilla_vec3i_with_a_red_icon() {
        let bytes = encode(TrackedWaypoint {
            operation: WaypointOperation::Track,
            waypoint: Waypoint {
                id: id(),
                icon: WaypointIcon {
                    style: DEFAULT_WAYPOINT_STYLE.to_string(),
                    color: Some(WaypointColor::rgb(0xFF0000)),
                },
                position: WaypointPosition::Block {
                    x: 10,
                    y: 64,
                    z: -5,
                },
            },
        });
        let expected = hex("8a 01
             00
             01 00000000000000010000000000000002
             11 6d696e6563726166743a64656661756c74
             01 ff0000
             01 0a 40 fbffffff0f");
        assert_eq!(bytes, expected);
        assert_eq!(bytes.len() - 2, 48);
    }

    #[test]
    fn untrack_carries_the_empty_default_body() {
        let bytes = encode(TrackedWaypoint {
            operation: WaypointOperation::Untrack,
            waypoint: Waypoint::untracked(id()),
        });
        let expected = hex("8a 01
             01
             01 00000000000000010000000000000002
             11 6d696e6563726166743a64656661756c74
             00
             00");
        assert_eq!(bytes, expected);
    }

    #[test]
    fn named_chunk_and_azimuth_waypoints() {
        let mut bytes = Vec::new();
        Waypoint {
            id: WaypointId::Name("home".into()),
            icon: WaypointIcon::default(),
            position: WaypointPosition::Chunk { x: -1, z: 3 },
        }
        .encode(&mut bytes);
        let mut expected = vec![0x00, 0x04];
        expected.extend_from_slice(b"home");
        expected.push(0x11);
        expected.extend_from_slice(DEFAULT_WAYPOINT_STYLE.as_bytes());
        expected.extend_from_slice(&[0x00, 0x02, 0xff, 0xff, 0xff, 0xff, 0x0f, 0x03]);
        assert_eq!(bytes, expected);

        let mut azimuth = Vec::new();
        WaypointPosition::Azimuth(1.5).encode(&mut azimuth);
        let mut expected = vec![0x03];
        expected.extend_from_slice(&1.5f32.to_be_bytes());
        assert_eq!(azimuth, expected);
    }

    #[test]
    fn round_trips() {
        for position in [
            WaypointPosition::Empty,
            WaypointPosition::Block {
                x: -300,
                y: 70,
                z: 9,
            },
            WaypointPosition::Chunk { x: 4, z: -4 },
            WaypointPosition::Azimuth(-0.25),
        ] {
            let packet = TrackedWaypoint {
                operation: WaypointOperation::Update,
                waypoint: Waypoint {
                    id: id(),
                    icon: WaypointIcon {
                        style: "minecraft:bowtie".into(),
                        color: Some(WaypointColor::rgb(0x12_34_56)),
                    },
                    position,
                },
            };
            let mut bytes = Vec::new();
            packet.encode(&mut bytes);
            let decoded = TrackedWaypoint::decode(&mut bytes.as_slice()).expect("decode");
            assert_eq!(decoded, packet);
        }
    }
}
